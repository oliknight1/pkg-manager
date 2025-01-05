
# pkg-manager

This is a simple package manager implemented in Rust, primarily created as a learning project. It aims to mimic some of the basic functionalities of package managers like npm.

## Key Features
Dependency Resolution: The package manager resolves dependencies defined in a package.json file.
Lock File: It supports lock files (dep-lock.json) for consistent versioning and dependency management.
Tarball Fetching: Dependencies are fetched as .tar.gz archives from a registry (e.g., npm) and extracted into a local node_modules folder.
Integrity Check: Ensures that downloaded dependencies match their integrity hash to verify their authenticity.
Version Conflict Handling: Basic handling for version conflicts, where a dependency may require different versions of the same library.


## Installation
### Prerequisites
Rust (1.50.0 or newer) installed on your machine.
cargo (comes with Rust installation) to build and run the project.

### Building the Project
To build the project, clone this repository and run the following command inside the project directory:

```bash
cargo build --release
```
### Running the Project
To run the project, you need to create a package.json file that lists your dependencies, then execute:

```bash
cargo run
```

This will:

- Read the package.json to find the dependencies.
- Fetch the required dependencies from the registry (currently npm).
-  Store the resolved dependencies in node_modules and update the dep-lock.json with the resolved versions.

## How It Works

### Dependencies in package.json:

The dependencies in package.json are read by the package manager.
The manager will attempt to resolve the versions specified, checking if they match the versions already stored in the lock file (dep-lock.json).
### Lock File:

If a lock file (dep-lock.json) exists, the manager will check if the dependencies and their versions match the ones in the lock file. If they do, it fetches the package from the URL stored in the lock file.
If the versions don't match, the manager will fetch the latest matching versions and update the lock file.

### Tarball Fetching:
Dependencies are fetched as .tar.gz archives from the registry (e.g., npm) and unpacked into the node_modules folder.

### Integrity Checking:
When fetching dependencies, the integrity hash is checked against the hash provided by the registry to ensure the authenticity of the packages.
### Version Conflicts:
If dependencies require different versions of the same library, the package manager handles the conflict by installing the required versions in their own directories under node_modules.

### Limitations
This package manager is not a fully-featured or production-ready package manager.
Some edge cases, such as dependency resolution involving complex version ranges, are not fully handled.
The project does not handle features like devDependencies, package scripts, or other npm-specific features.
