# Workspace migrations

This directory documents and develops migration operations for xws and
workspaces managed by xws. It is intended for changes that deliberately
transform existing workspace data, rather than routine behavior performed
when a workspace is created or activated.

A migration is generally a rare, explicit operation performed in a user's
environment. Examples include:

- Migrating workspaces from the legacy `ws` tool to xws.
- Updating an existing xws workspace to use a newly available feature or
  profile.
- Adapting workspaces to changes introduced by a newer xws release.
- Migrating workspaces when a major release changes backward compatibility.

Each migration can have different prerequisites, discovery rules, user
choices, and data-safety requirements. Keep its design, implementation, and
relevant usage notes in a dedicated subdirectory. Document what the migration
will change and how it handles existing data before implementing an operation
that modifies user workspaces.

Migrations should be explicit and understandable: report what was discovered,
what will change, and what happened. Avoid overwriting or discarding user data
without clear intent. Where an operation can fail partway through, document
how to identify the resulting state and safely retry or recover.

See [`workspace/`](workspace/) for the generic workspace migration engine and
its legacy `ws` adapter.
