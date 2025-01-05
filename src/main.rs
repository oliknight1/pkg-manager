use base64::engine::general_purpose;
use base64::Engine;
use flate2::read::GzDecoder;
use reqwest::{blocking::Client, Error};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::path::Path;
use std::{collections::HashMap, fs, io::Cursor};
use tar::Archive;

#[derive(Deserialize, Debug)]
struct PackageJSON {
    // name: String,
    // description: String,
    dependencies: Option<HashMap<String, String>>,
}
#[derive(Deserialize, Debug)]
struct RegistryResponse {
    versions: HashMap<String, RegistryVersionItem>,
}

#[derive(Deserialize, Debug, Clone)]
struct RegistryVersionItem {
    version: String,
    dist: RegistryDist,
    dependencies: Option<HashMap<String, String>>,
    // #[serde(rename = "devDependencies")]
    // dev_dependencies: HashMap<String, String>,
}

#[derive(Deserialize, Debug, Clone)]
struct RegistryDist {
    integrity: String,
    tarball: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct LockFileItem {
    version: String,
    resolved_url: String,
    integrity: String,
    dependencies: Option<HashMap<String, String>>,
}
type LockFile = HashMap<String, LockFileItem>;

//TODO: Handle devDependencies

//TODO: Start with on demand dependency resolution, then switch to a different data structure.
// Maybe a tree or a Directed Acylic Graph

const LOCK_FILE_PATH: &str = "dep-lock.json";
const PACKAGE_JSON_PATH: &str = "package.json";

fn main() {
    let client = Client::new();
    let package_json = fs::read_to_string(PACKAGE_JSON_PATH).expect("Error reading file");

    let package_json: PackageJSON =
        serde_json::from_str(&package_json).expect("Error reading json");

    let mut lock_file: LockFile = if Path::new(LOCK_FILE_PATH).exists() {
        let lock_content = fs::read_to_string(LOCK_FILE_PATH).expect("Error reading lock file");
        serde_json::from_str(&lock_content).expect("Error parsing lock file")
    } else {
        HashMap::new()
    };

    match package_json.dependencies {
        Some(deps) => {
            if let Err(e) = fetch_dependencies(deps, &client, &mut lock_file, None) {
                eprintln!("Error: {e}")
            }
        }
        None => println!("No dependencies"),
    }

    if let Err(e) = write_lock_file(&lock_file) {
        eprintln!("Failed to write to lock file: {e}")
    }
}

fn write_lock_file(lock_file: &LockFile) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(lock_file)?;
    fs::write(LOCK_FILE_PATH, json)?;
    println!("Wrote lock file");
    Ok(())
}

/// Fetches single dependency from registry
fn fetch_single_dep(
    name: &String,
    version: &String,
    client: &Client,
    lock_file: &mut LockFile,
    parent_node_modules: Option<&String>, // Parent directory for nested `node_modules`
) -> Result<(), Box<dyn std::error::Error>> {
    let dependency_folder = match parent_node_modules {
        Some(parent) => format!("{}/node_modules", parent),
        None => "./node_modules".to_string(),
    };

    // Check if dependency exists in the lock file
    if let Some(lock_item) = lock_file.get(name) {
        let package_version = VersionReq::parse(version)
            .expect("Failed to parse dependency version from package.json");

        let lock_version = Version::parse(&lock_item.version)
            .expect("Failed to parse dependency version from dep-lock.json");

        if package_version.matches(&lock_version) {
            // Dependency already resolved and matches the required version
            fetch_tarball(
                &lock_item.resolved_url,
                name,
                client,
                Some(lock_item.integrity.clone()),
                &dependency_folder,
            )?;
            if let Some(deps) = &lock_item.dependencies {
                fetch_dependencies(deps.clone(), client, lock_file, Some(&dependency_folder))?;
            }
            return Ok(());
        }
    }

    // Fetch the latest compatible version
    let matched_dependency = get_latest_version(name, version, client)?;
    println!("Matched: {:?}", matched_dependency);

    let integrity = matched_dependency.dist.integrity.clone();
    fetch_tarball(
        &matched_dependency.dist.tarball,
        name,
        client,
        Some(integrity.clone()),
        &dependency_folder,
    )?;

    if let Some(deps) = &matched_dependency.dependencies {
        fetch_dependencies(deps.clone(), client, lock_file, Some(&dependency_folder))?;
    }

    // Update the lock file
    lock_file.insert(
        name.to_string(),
        LockFileItem {
            version: matched_dependency.version,
            resolved_url: matched_dependency.dist.tarball,
            integrity,
            dependencies: matched_dependency.dependencies.clone(),
        },
    );

    Ok(())
}

fn fetch_dependencies(
    dependencies: HashMap<String, String>,
    client: &Client,
    lock_file: &mut LockFile,
    parent_node_modules: Option<&String>, // Parent directory for nested `node_modules`
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, version) in dependencies {
        fetch_single_dep(&name, &version, client, lock_file, parent_node_modules)?;
    }
    Ok(())
}

fn fetch_tarball(
    url: &String,
    name: &String,
    client: &Client,
    expected_integrity: Option<String>,
    output_dir: &String, // Directory where the dependency will be installed
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client.get(url).send()?;
    let content_bytes = response.bytes()?;

    if let Some(expected_hash) = expected_integrity {
        let parts: Vec<&str> = expected_hash.split('-').collect();
        if parts.len() != 2 || parts[0] != "sha512" {
            return Err(format!("Unsupported hash algorithm in {expected_hash}").into());
        }
        let expected_hash_value = parts[1];

        let mut hasher = Sha512::new();
        hasher.update(&content_bytes);
        let computed_hash = general_purpose::STANDARD.encode(hasher.finalize());

        if expected_hash_value != computed_hash {
            return Err(format!(
                "Integrity check failed for {name}. Expected {expected_hash_value}, got {computed_hash}"
            )
            .into());
        }
        println!("Integrity check passed for {name}");
    } else {
        println!("No integrity hash provided for {name}. Skipping validation.");
    }

    let cursor = Cursor::new(&content_bytes);
    let tar = GzDecoder::new(cursor);
    let mut archive = Archive::new(tar);

    // Unpack the tarball
    let dependency_path = format!("{}/{}", output_dir, name);
    archive.unpack(&dependency_path)?;
    Ok(())
}

///  Parses the dependency version parsed from package.json. If the dependency is a range, find the latest version
///
/// * `name`: name of the dependency
/// * `version`: version specified in the package.json
/// * `client`: reqwest client
fn get_latest_version(
    name: &String,
    version: &String,
    client: &Client,
) -> Result<RegistryVersionItem, Error> {
    let url = format!("https://registry.npmjs.org/{}", name);
    let response = client.get(&url).send();
    match response {
        Ok(data) => {
            let body = data.text().expect("Error reading body");

            let json: RegistryResponse = serde_json::from_str(&body).expect("Error reading json");
            let versions: Vec<String> = json.versions.keys().cloned().collect();

            if versions.contains(version) {
                return Ok(json.versions[version].clone());
            }
            // PERF: might be a faster way to do this instead of fetching the whole list of dependency version
            let version_req = VersionReq::parse(version).unwrap();

            let mut matching_versions: Vec<Version> = versions
                .iter()
                .filter_map(|version| Version::parse(version).ok())
                .filter(|version| version_req.matches(version))
                .collect();

            matching_versions.sort();

            let latest_version = matching_versions
                .last()
                .map(|version| version.to_string())
                .expect("No matching package found");

            Ok(json.versions[&latest_version].clone())
        }
        Err(e) => {
            //NOTE: enhance here with retries
            Err(e)
        }
    }
}
