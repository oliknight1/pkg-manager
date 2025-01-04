use flate2::read::GzDecoder;
use reqwest::{blocking::Client, Error};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::fs::File;
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
}

#[derive(Deserialize, Debug, Clone)]
struct RegistryDist {
    tarball: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct LockFileItem {
    version: String,
    resolved_url: String,
    integrity: String,
    dependencies: Vec<String>,
}
type LockFile = HashMap<String, LockFileItem>;

//TODO: 2. handle nested dependencies

//TODO: 3. handle different dependency conflicts

//TODO: 4. Start with on demand dependency resolution, then switch to a different data structure.
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
            if let Err(e) = parse_full_dep_list(deps, &client, &mut lock_file) {
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
) -> Result<(), Box<dyn std::error::Error>> {
    // Check if dependency exits in lock file
    if let Some(lock_item) = lock_file.get(name) {
        let package_version = VersionReq::parse(version)
            .expect("Failed to parse dependency version from package.json");

        let lock_version = Version::parse(&lock_item.version)
            .expect("Failed to parse dependency version from dep-lock.json");

        if package_version.matches(&lock_version) {
            fetch_tarball(&lock_item.resolved_url, name, client)?;
            return Ok(());
        }
    }
    let matched_version = get_latest_version(name, version, client)?;
    fetch_tarball(&matched_version.dist.tarball, name, client)?;

    lock_file.insert(
        name.to_string(),
        LockFileItem {
            version: matched_version.version,
            resolved_url: matched_version.dist.tarball,
            integrity: "integrity-placeholder".to_string(),
            dependencies: Vec::new(),
        },
    );

    Ok(())
}

fn parse_full_dep_list(
    dependencies: HashMap<String, String>,
    client: &Client,
    lock_file: &mut LockFile,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, version) in dependencies {
        fetch_single_dep(&name, &version, client, lock_file)?
    }
    Ok(())
}

fn fetch_tarball(
    url: &String,
    name: &String,
    client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client.get(url).send()?;

    let content = Cursor::new(response.bytes()?);

    let tar = GzDecoder::new(content);
    let mut archive = Archive::new(tar);
    let output_dir = format!("./node_modules/{name}");

    //NOTE: enhance here to remove a root directory if it has a single dir as the root
    archive.unpack(output_dir)?;
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
