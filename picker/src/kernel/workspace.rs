use std::collections::{BTreeSet, HashSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use log::warn;
use serde::Deserialize;

pub use super::validation::{validate_environment_name, validate_name};

struct ResolvedTemplates {
    activate: Option<PathBuf>,
    deactivate: Option<PathBuf>,
    once: Option<PathBuf>,
    env: Option<PathBuf>,
}

struct ResolvedProfile {
    name: String,
    templates: ResolvedTemplates,
}

struct ResolvedFeature {
    name: String,
    templates: ResolvedTemplates,
}

#[derive(Clone, Copy)]
enum TemplateKind {
    Profile,
    Feature,
}

impl TemplateKind {
    fn directory(self) -> &'static str {
        match self {
            Self::Profile => "profiles",
            Self::Feature => "features",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::Feature => "feature",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureManifest {
    #[serde(rename = "with-profiles", default)]
    with_profiles: Option<Vec<String>>,
}

/// Reports whether a workspace is in use by another shell; `Err` blocks the operation.
pub type OtherShellsGuard<'a> = &'a dyn Fn(&str) -> Result<(), Box<dyn Error>>;

/// The live activation context as observed by the shell.
///
/// This is a snapshot of the `XWS_WORKSPACE_PATH` environment variable: the
/// directory the shell currently considers active. It is read once at the
/// process edge and injected into the store methods that need it, rather than
/// read from `std::env` at call time, so the active-workspace guard stays a
/// single, testable policy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveContext {
    workspace_path: Option<PathBuf>,
}

impl ActiveContext {
    /// Read the active context from the environment.
    pub fn from_env() -> Self {
        Self {
            workspace_path: std::env::var_os("XWS_WORKSPACE_PATH").map(PathBuf::from),
        }
    }

    /// Is `path` the currently active workspace directory?
    ///
    /// Compares the two paths by their canonical (real) form, so a trailing
    /// slash, `.`/`..` components, or a symlinked `prefix` do not let an
    /// equivalent `XWS_WORKSPACE_PATH` slip past the "deactivate before
    /// remove/move" guard. If either path cannot be canonicalized (for example
    /// the active path is dangling), fall back to a lexical comparison so a
    /// transient filesystem error never blocks the operation; this is a UX
    /// guard, not a security boundary.
    pub fn is_active(&self, path: &Path) -> bool {
        let Some(active) = &self.workspace_path else {
            return false;
        };
        match (fs::canonicalize(path), fs::canonicalize(active)) {
            (Ok(canonical_path), Ok(canonical_active)) => canonical_path == canonical_active,
            _ => Path::new(active) == path,
        }
    }
}

pub struct WorkspaceStore {
    prefix: PathBuf,
}

impl WorkspaceStore {
    pub fn from_environment() -> Result<Self, Box<dyn Error>> {
        let prefix = match std::env::var_os("XWS_PREFIX") {
            Some(path) => PathBuf::from(path),
            None => {
                PathBuf::from(std::env::var_os("HOME").ok_or("HOME is not set; set XWS_PREFIX")?)
                    .join("xws")
            }
        };
        Ok(Self { prefix })
    }

    pub fn workspaces_dir(&self) -> PathBuf {
        self.prefix.join("workspaces")
    }

    pub fn profiles(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.bundle_names("profiles")
    }

    pub fn features(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.bundle_names("features")
    }

    pub fn list(&self) -> Result<Vec<String>, Box<dyn Error>> {
        let root = self.workspaces_dir();
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut workspaces = Vec::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                warn!(
                    "xws: skipping workspace directory with a non-UTF-8 name: {}",
                    entry.path().display()
                );
                continue;
            };
            if let Err(error) = validate_name(&name) {
                warn!("xws: skipping workspace directory: {error}");
                continue;
            }
            workspaces.push(name);
        }
        workspaces.sort();
        Ok(workspaces)
    }

    fn bundle_names(&self, category: &str) -> Result<Vec<String>, Box<dyn Error>> {
        let packaged_root = packaged_templates_root()?.join(category);
        let user_root = self.prefix.join("templates").join(category);
        let mut names = BTreeSet::new();
        for root in [&packaged_root, &user_root] {
            let entries = match fs::read_dir(root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| format!("bundle name is not valid UTF-8: {}", root.display()))?;
                match category {
                    "profiles" | "features" => validate_name(&name)?,
                    _ => return Err(format!("unknown template category '{category}'").into()),
                }
                names.insert(name);
            }
        }
        Ok(names.into_iter().collect())
    }

    pub fn workspace_path(&self, name: &str) -> Result<PathBuf, Box<dyn Error>> {
        validate_name(name)?;
        let path = self.workspaces_dir().join(name);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| format!("workspace '{name}' does not exist"))?;
        if !metadata.file_type().is_dir() {
            return Err(format!("workspace '{name}' is not a directory").into());
        }
        Ok(path)
    }

    pub fn environment(&self, name: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
        let path = self.workspace_path(name)?.join("bin/env");
        let contents = fs::read_to_string(&path).map_err(|error| {
            format!(
                "cannot read workspace environment file {}: {error}",
                path.display()
            )
        })?;
        parse_environment(&path, &contents)
    }

    pub fn create(
        &self,
        name: &str,
        profiles: &[String],
        features: &[String],
    ) -> Result<(), Box<dyn Error>> {
        validate_name(name)?;
        let default_profile = self.resolve_profile("default")?;
        let default_activate = default_profile
            .templates
            .activate
            .as_ref()
            .ok_or("default profile is missing activate template")?;
        let default_deactivate = default_profile
            .templates
            .deactivate
            .as_ref()
            .ok_or("default profile is missing deactivate template")?;
        let default_once = default_profile
            .templates
            .once
            .as_ref()
            .ok_or("default profile is missing once template")?;
        let mut selected_profiles = HashSet::new();
        let mut profiles = profiles.to_vec();
        for profile in &profiles {
            validate_name(profile)?;
            if !selected_profiles.insert(profile) {
                return Err(format!("profile '{profile}' was selected more than once").into());
            }
        }
        profiles.retain(|profile| profile != "default");
        profiles.sort();
        let profile_templates = profiles
            .iter()
            .map(|profile| self.resolve_profile(profile))
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
        let mut features = features.to_vec();
        features.sort();
        let feature_templates = self.resolve_features(&features, &profiles)?;
        let root = self.workspaces_dir();
        let workspace = root.join(name);
        let env_path = workspace.join("bin/env");
        let mut env_contents = String::from("# NAME=value; literal exported variables\n");
        for profile in std::iter::once(&default_profile).chain(&profile_templates) {
            if let Some(template) = &profile.templates.env {
                append_environment_template(&mut env_contents, template)?;
            }
        }
        for feature in &feature_templates {
            if let Some(template) = &feature.templates.env {
                append_environment_template(&mut env_contents, template)?;
            }
        }
        parse_environment(&env_path, &env_contents)?;

        fs::create_dir_all(&root)?;
        fs::create_dir(&workspace)
            .map_err(|error| format!("cannot create workspace '{name}': {error}"))?;

        let result = (|| -> Result<(), Box<dyn Error>> {
            let bin = workspace.join("bin");
            fs::create_dir(&bin)?;
            fs::create_dir(workspace.join("src"))?;
            fs::create_dir(bin.join("activate.d"))?;
            fs::create_dir(bin.join("deactivate.d"))?;
            fs::write(&env_path, &env_contents)?;

            for (hook, template) in [
                ("activate", default_activate),
                ("deactivate", default_deactivate),
            ] {
                fs::copy(template, bin.join(hook))
                    .map_err(|error| format!("cannot copy {}: {error}", template.display()))?;
            }
            let mut once_script = Vec::new();
            append_script(&mut once_script, default_once)?;
            for profile in &profile_templates {
                if let Some(template) = &profile.templates.once {
                    append_script(&mut once_script, template).map_err(|error| {
                        format!(
                            "cannot append profile '{}' once script: {error}",
                            profile.name
                        )
                    })?;
                }
            }
            for feature in &feature_templates {
                if let Some(template) = &feature.templates.once {
                    append_script(&mut once_script, template).map_err(|error| {
                        format!(
                            "cannot append feature '{}' once script: {error}",
                            feature.name
                        )
                    })?;
                }
            }
            fs::write(bin.join("once"), once_script)?;

            let profile_count = profile_templates.len();
            for (index, profile) in profile_templates.iter().enumerate() {
                if let Some(template) = &profile.templates.activate {
                    let filename = format!("10-profile-{index:020}");
                    fs::copy(template, bin.join("activate.d").join(&filename))?;
                }
                let reverse_index = profile_count - index - 1;
                if let Some(template) = &profile.templates.deactivate {
                    let filename = format!("20-profile-{reverse_index:020}");
                    fs::copy(template, bin.join("deactivate.d").join(filename))?;
                }
            }
            for feature in &feature_templates {
                if let Some(template) = &feature.templates.activate {
                    let filename = format!("20-feature-{}", feature.name);
                    fs::copy(template, bin.join("activate.d").join(&filename))?;
                }
                if let Some(template) = &feature.templates.deactivate {
                    let filename = format!("10-feature-{}", feature.name);
                    fs::copy(template, bin.join("deactivate.d").join(filename))?;
                }
            }

            let status = Command::new("bash")
                .arg(bin.join("once"))
                .current_dir(&workspace)
                .env("XWS_PREFIX", &self.prefix)
                .env("XWS_WORKSPACE", name)
                .env("XWS_WORKSPACE_PATH", &workspace)
                .status()
                .map_err(|error| format!("could not run workspace once script: {error}"))?;
            if !status.success() {
                return Err(format!(
                    "workspace files were created, but its once script exited with {status}; inspect {}",
                    bin.join("once").display()
                )
                .into());
            }
            Ok(())
        })();

        result?;
        Ok(())
    }

    /// Attempt to remove each workspace by name, returning one result per
    /// input name, in order. A result is `Err` when the name does not
    /// resolve, when the workspace is active here or (per `other_shells`) in
    /// another shell, or when the directory cannot be removed. Presentation (printing and exit-code mapping) is
    /// left to the caller so the per-item outcome is not collapsed.
    pub fn remove_many(
        &self,
        active: &ActiveContext,
        other_shells: OtherShellsGuard<'_>,
        names: &[String],
    ) -> Vec<Result<(), Box<dyn Error>>> {
        names
            .iter()
            .map(|name| {
                let path = self.workspace_path(name)?;
                if active.is_active(&path) {
                    return Err("deactivate the workspace before removing it"
                        .to_string()
                        .into());
                }
                other_shells(name)?;
                fs::remove_dir_all(&path)?;
                Ok(())
            })
            .collect()
    }

    pub fn move_workspace(
        &self,
        active: &ActiveContext,
        other_shells: OtherShellsGuard<'_>,
        source: &str,
        target: &str,
    ) -> Result<(), Box<dyn Error>> {
        let source_path = self.workspace_path(source)?;
        validate_name(target)?;
        let target_path = self.workspaces_dir().join(target);

        if active.is_active(&source_path) {
            return Err("deactivate the source workspace before moving it".into());
        }
        other_shells(source)?;
        if fs::symlink_metadata(&target_path).is_ok() {
            return Err(format!("target workspace already exists: {target}").into());
        }
        fs::rename(source_path, target_path)?;
        Ok(())
    }

    fn resolve_profile(&self, profile: &str) -> Result<ResolvedProfile, Box<dyn Error>> {
        validate_name(profile)?;
        if !self
            .profiles()?
            .iter()
            .any(|available| available == profile)
        {
            return Err(format!("unknown profile '{profile}'").into());
        }

        Ok(ResolvedProfile {
            name: profile.to_owned(),
            templates: self.resolve_templates(TemplateKind::Profile, profile)?,
        })
    }

    fn resolve_templates(
        &self,
        kind: TemplateKind,
        name: &str,
    ) -> Result<ResolvedTemplates, Box<dyn Error>> {
        let activate = self.resolve_bundle_template(kind, name, "activate")?;
        let deactivate = self.resolve_bundle_template(kind, name, "deactivate")?;
        let once = self.resolve_bundle_template(kind, name, "once")?;
        let env = self.resolve_bundle_template(kind, name, "env")?;

        if activate.is_some() != deactivate.is_some() {
            return Err(format!(
                "{} '{name}' must provide activate and deactivate templates together",
                kind.label()
            )
            .into());
        }
        if activate.is_none() && once.is_none() && env.is_none() {
            return Err(format!("{} '{name}' has no templates", kind.label()).into());
        }

        Ok(ResolvedTemplates {
            activate,
            deactivate,
            once,
            env,
        })
    }

    fn resolve_bundle_template(
        &self,
        kind: TemplateKind,
        name: &str,
        template: &str,
    ) -> Result<Option<PathBuf>, Box<dyn Error>> {
        let user_path = self
            .prefix
            .join("templates")
            .join(kind.directory())
            .join(name)
            .join(template);
        if let Some(path) = existing_regular_file(&user_path)? {
            return Ok(Some(path));
        }

        let packaged_path = packaged_templates_root()?
            .join(kind.directory())
            .join(name)
            .join(template);
        existing_regular_file(&packaged_path)
    }

    fn resolve_features(
        &self,
        features: &[String],
        profiles: &[String],
    ) -> Result<Vec<ResolvedFeature>, Box<dyn Error>> {
        let mut seen = HashSet::new();
        let mut resolved = Vec::new();
        let available_features = self.features()?;
        for feature in features {
            validate_name(feature)?;
            if !seen.insert(feature) {
                return Err(format!("feature '{feature}' was selected more than once").into());
            }
            if !available_features
                .iter()
                .any(|available| available == feature)
            {
                return Err(format!("unknown feature '{feature}'").into());
            }

            let manifest_path = self
                .resolve_bundle_template(TemplateKind::Feature, feature, "manifest.toml")?
                .ok_or_else(|| format!("feature '{feature}' is missing manifest.toml"))?;
            let manifest_contents = fs::read_to_string(&manifest_path).map_err(|error| {
                format!(
                    "cannot read feature '{feature}' manifest {}: {error}",
                    manifest_path.display()
                )
            })?;
            let manifest: FeatureManifest =
                toml::from_str(&manifest_contents).map_err(|error| {
                    format!(
                        "invalid feature '{feature}' manifest {}: {error}",
                        manifest_path.display()
                    )
                })?;
            validate_feature_profile_constraints(feature, &manifest, profiles)?;

            resolved.push(ResolvedFeature {
                name: feature.clone(),
                templates: self.resolve_templates(TemplateKind::Feature, feature)?,
            });
        }
        Ok(resolved)
    }
}

fn validate_feature_profile_constraints(
    feature: &str,
    manifest: &FeatureManifest,
    profiles: &[String],
) -> Result<(), Box<dyn Error>> {
    let Some(allowed_profiles) = &manifest.with_profiles else {
        return Ok(());
    };

    if allowed_profiles.is_empty() {
        return Err(format!("feature '{feature}' manifest with-profiles must not be empty").into());
    }

    let mut allowed = HashSet::new();
    for profile in allowed_profiles {
        validate_name(profile).map_err(|_| {
            format!("feature '{feature}' manifest contains invalid profile name: {profile:?}")
        })?;
        if profile == "default" || !allowed.insert(profile) {
            return Err(format!(
                "feature '{feature}' manifest has an invalid or duplicate with-profiles entry: {profile}"
            )
            .into());
        }
    }

    if !profiles
        .iter()
        .any(|profile| allowed_profiles.contains(profile))
    {
        return Err(format!(
            "feature '{feature}' requires one of these profiles: {}",
            allowed_profiles.join(", ")
        )
        .into());
    }

    Ok(())
}

fn existing_regular_file(path: &Path) -> Result<Option<PathBuf>, Box<dyn Error>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(Some(path.to_path_buf())),
        Ok(_) => Err(format!("template is not a regular file: {}", path.display()).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("cannot inspect template {}: {error}", path.display()).into()),
    }
}

/// Locate the packaged templates directory (the one containing `profiles/`
/// and `features/`), in this order:
///
/// 1. `XWS_TEMPLATES_DIR`, used as-is; an invalid value is an error rather
///    than a silent fall-through.
/// 2. `<exe>/../share/xws/templates`.
/// 3. `<repo>/templates` via `CARGO_MANIFEST_DIR`, debug builds only, so a
///    release binary never embeds or trusts its build machine's source path.
fn packaged_templates_root() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(value) = std::env::var_os("XWS_TEMPLATES_DIR") {
        let root = PathBuf::from(value);
        if !root.is_dir() {
            return Err(format!("XWS_TEMPLATES_DIR is not a directory: {}", root.display()).into());
        }
        return Ok(root);
    }

    let executable = std::env::current_exe()?;
    let installed_root = executable
        .parent()
        .and_then(Path::parent)
        .map(|prefix| prefix.join("share/xws/templates"));
    if let Some(root) = installed_root.filter(|path| path.is_dir()) {
        return Ok(root);
    }

    #[cfg(debug_assertions)]
    {
        let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("templates");
        if checkout.is_dir() {
            return Ok(checkout);
        }
    }

    Err("xws templates not found next to the binary; reinstall xws or set XWS_TEMPLATES_DIR".into())
}

fn append_script(script: &mut Vec<u8>, template: &Path) -> Result<(), Box<dyn Error>> {
    let contents = fs::read(template)
        .map_err(|error| format!("cannot read {}: {error}", template.display()))?;
    script.extend_from_slice(&contents);
    if !contents.ends_with(b"\n") {
        script.push(b'\n');
    }
    Ok(())
}

fn append_environment_template(
    environment: &mut String,
    template: &Path,
) -> Result<(), Box<dyn Error>> {
    let contents = fs::read_to_string(template).map_err(|error| {
        format!(
            "cannot read environment template {}: {error}",
            template.display()
        )
    })?;
    if !environment.ends_with('\n') {
        environment.push('\n');
    }
    environment.push_str(&contents);
    if !contents.is_empty() && !contents.ends_with('\n') {
        environment.push('\n');
    }
    Ok(())
}

fn parse_environment(path: &Path, contents: &str) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut variables = Vec::new();
    let mut seen = HashSet::new();
    for (index, line) in contents.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("{}:{}: expected NAME=value", path.display(), index + 1))?;
        validate_environment_name(key)?;
        if value.chars().any(char::is_control) {
            return Err(format!(
                "{}:{}: environment values cannot contain control characters",
                path.display(),
                index + 1
            )
            .into());
        }
        if !seen.insert(key.to_owned()) {
            return Err(format!(
                "{}:{}: duplicate environment variable '{key}'",
                path.display(),
                index + 1
            )
            .into());
        }
        variables.push((key.to_owned(), value.to_owned()));
    }
    Ok(variables)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_and_lists_a_workspace_with_simple_name() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store.create("project-1", &[], &[]).unwrap();

        let listing = store.list().unwrap();
        assert_eq!(listing, ["project-1"]);
        assert!(
            store
                .workspace_path("project-1")
                .unwrap()
                .join("src")
                .is_dir()
        );
    }

    #[test]
    fn user_template_overrides_only_its_matching_packaged_hook() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        let user_templates = temp.path().join("templates/profiles/default");
        fs::create_dir_all(&user_templates).unwrap();
        fs::write(user_templates.join("activate"), "# custom activation\n").unwrap();

        store.create("customized", &[], &[]).unwrap();

        let bin = store.workspace_path("customized").unwrap().join("bin");
        assert_eq!(
            fs::read_to_string(bin.join("activate")).unwrap(),
            "# custom activation\n"
        );
        assert!(bin.join("deactivate").is_file());
        assert!(bin.join("once").is_file());
    }

    #[test]
    fn selected_feature_templates_are_copied_into_activation_directories() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store
            .create("with-go", &[], &["golang".to_owned()])
            .unwrap();
        let bin = store.workspace_path("with-go").unwrap().join("bin");
        assert!(bin.join("activate.d/20-feature-golang").is_file());
        assert!(bin.join("deactivate.d/10-feature-golang").is_file());
    }

    #[test]
    fn feature_manifest_rejects_unknown_fields() {
        assert!(toml::from_str::<FeatureManifest>("with-profile = \"java\"").is_err());
        assert!(toml::from_str::<FeatureManifest>("unknown = true").is_err());
    }

    #[test]
    fn active_context_matches_equivalent_path_spellings() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store.create("guard", &[], &[]).unwrap();
        let workspace = store.workspace_path("guard").unwrap();

        // A trailing-slash spelling of the active path must still match,
        // since both sides are compared by their canonical form.
        let trailing_slash = PathBuf::from(format!("{}/", workspace.to_string_lossy()));
        let active = ActiveContext {
            workspace_path: Some(trailing_slash),
        };
        assert!(
            active.is_active(&workspace),
            "trailing-slash active path should match"
        );
        assert!(
            !ActiveContext::default().is_active(&workspace),
            "no active context should never match"
        );
    }

    #[test]
    fn remove_refuses_active_workspace_set_via_trailing_slash() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store.create("guard", &[], &[]).unwrap();
        let workspace = store.workspace_path("guard").unwrap();

        // Simulate a shell that stored an equivalent-but-differently-spelled
        // active path; the guard must still fire.
        let active = ActiveContext {
            workspace_path: Some(PathBuf::from(format!("{}/", workspace.to_string_lossy()))),
        };
        assert!(
            store
                .remove_many(&active, &|_| Ok(()), &["guard".to_owned()])
                .into_iter()
                .any(|result| result.is_err())
        );
        assert!(workspace.exists(), "active workspace must not be removed");

        // With no active context, the same removal succeeds.
        assert!(
            !store
                .remove_many(
                    &ActiveContext::default(),
                    &|_| Ok(()),
                    &["guard".to_owned()]
                )
                .into_iter()
                .any(|result| result.is_err())
        );
        assert!(!workspace.exists());
    }

    #[test]
    fn remove_and_move_consult_the_other_shells_guard() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store.create("held", &[], &[]).unwrap();
        store.create("free", &[], &[]).unwrap();
        let guard = |name: &str| -> Result<(), Box<dyn Error>> {
            if name == "held" {
                Err("active elsewhere".into())
            } else {
                Ok(())
            }
        };
        let active = ActiveContext::default();

        let outcomes = store.remove_many(&active, &guard, &["held".to_owned(), "free".to_owned()]);
        assert!(outcomes[0].is_err());
        assert!(outcomes[1].is_ok());
        assert!(store.workspace_path("held").is_ok());
        assert!(store.workspace_path("free").is_err());

        assert!(
            store
                .move_workspace(&active, &guard, "held", "moved")
                .is_err()
        );
        assert!(store.workspace_path("held").is_ok());
        store.create("free2", &[], &[]).unwrap();
        store
            .move_workspace(&active, &guard, "free2", "moved")
            .unwrap();
        assert!(store.workspace_path("moved").is_ok());
    }

    #[test]
    fn remove_reports_each_failure_independently() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store.create("keep", &[], &[]).unwrap();
        store.create("remove-me", &[], &[]).unwrap();
        let keep = store.workspace_path("keep").unwrap();
        let remove_me = store.workspace_path("remove-me").unwrap();

        // "keep" is active, "missing" does not exist, "remove-me" is removable.
        let active = ActiveContext {
            workspace_path: Some(keep.clone()),
        };
        let outcomes = store.remove_many(
            &active,
            &|_| Ok(()),
            &[
                "keep".to_owned(),
                "missing".to_owned(),
                "remove-me".to_owned(),
            ],
        );

        assert_eq!(outcomes.len(), 3, "one result per input name, in order");
        assert!(outcomes[0].is_err(), "active workspace must fail");
        assert!(outcomes[1].is_err(), "missing workspace must fail");
        assert!(outcomes[2].is_ok(), "inactive workspace must be removed");
        assert!(keep.exists(), "active workspace must not be removed");
        assert!(!remove_me.exists(), "inactive workspace must be removed");
    }

    #[test]
    fn reads_declared_environment_and_rejects_reserved_names() {
        let temp = tempfile::tempdir().unwrap();
        let store = WorkspaceStore {
            prefix: temp.path().to_path_buf(),
        };
        store.create("env-test", &[], &[]).unwrap();
        fs::write(
            store.workspace_path("env-test").unwrap().join("bin/env"),
            "TOOL_MODE=workspace\nEMPTY=\n",
        )
        .unwrap();
        assert_eq!(
            store.environment("env-test").unwrap(),
            [
                ("TOOL_MODE".to_owned(), "workspace".to_owned()),
                ("EMPTY".to_owned(), "".to_owned())
            ]
        );
    }
}
