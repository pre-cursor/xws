use crate::kernel::workspace::ListWarning;

pub(super) fn report_workspace_warnings(warnings: &[ListWarning]) {
    for warning in warnings {
        match warning {
            ListWarning::NonUtf8Directory(path) => {
                eprintln!(
                    "xws: skipping workspace directory with a non-UTF-8 name: {}",
                    path.display()
                );
            }
            ListWarning::InvalidName(error) => {
                eprintln!("xws: skipping workspace directory: {error}");
            }
        }
    }
}
