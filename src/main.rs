use flate2::read::GzDecoder;
use reqwest::{blocking::Client, Error};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
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

//TODO: 1. Actually create the lock file

//TODO: 2. handle nested dependencies

//TODO: 3. handle different dependency conflicts

//TODO: 4. Start with on demand dependency resolution, then switch to a different data structure.
// Maybe a tree or a Directed Acylic Graph

fn main() {
    let client = Client::new();
    let json = fs::read_to_string("./package.json").expect("Error reading file");

    let json: PackageJSON = serde_json::from_str(&json).expect("Error reading json");

    println!("{:#?}", json);

    match json.dependencies {
        Some(deps) => {
            if Path::new("dep-lock.json").exists() {
                if let Err(e) = fetch_dep_from_lock(&client, deps) {
                    eprintln!("Error: {e}")
                };
            } else {
                if let Err(e) = parse_full_dep_list(deps, &client) {
                    eprintln!("Error: {e}")
                }
            }
        }
        None => println!("No dependencies"),
    }
}

/// fetch the dependency using the url from lock file
fn fetch_dep_from_lock(
    client: &Client,
    deps: HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let lock = fs::read_to_string("dep-lock.json")?;
    let lock: LockFile = serde_json::from_str(&lock)?;
    for (dep_name, lock_meta) in lock.iter() {
        //TODO: properly error hanlde
        let package_version = deps.get(dep_name);

        match package_version {
            Some(v) => {
                let package_version = VersionReq::parse(v)
                    .expect("Failed to parse dependency version from package.json");

                let lock_version = Version::parse(&lock_meta.version)
                    .expect("Failed to parse dependency version from dep-lock.json");

                let update_lock = !package_version.matches(&lock_version);

                // If the version in the lock file satisifies the version in package.json, then download using the
                // locked url. Else calculate the dependency version
                if !update_lock {
                    let url = lock_meta.resolved_url.clone();
                    println!("lock link: {url}");
                    if let Err(e) = fetch_tarball(&url, &dep_name.to_string(), client) {
                        return Err(format!("Error fetching tarball: {e}").into());
                    }
                    return Ok(());
                }
                fetch_dep(dep_name, &lock_meta.version, client)?
            }
            None => fetch_dep(dep_name, &lock_meta.version, client)?,
        }
    }
    Ok(())
}

/// Fetches single dependency from registry
fn fetch_dep(
    name: &String,
    version: &String,
    client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let matched_version = get_latest_version(name, version, client);
    match matched_version {
        Ok(mv) => {
            println!("matched version: {:?}", mv.dist.tarball);
            Ok(
                if let Err(e) = fetch_tarball(&mv.dist.tarball, name, client) {
                    return Err(format!("Error fetching tarball: {e}").into());
                },
            )
        }
        Err(e) => {
            return Err(format!("Error: {e}").into());
        }
    }
}

fn parse_full_dep_list(
    dependencies: HashMap<String, String>,
    client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, version) in dependencies {
        fetch_dep(&name, &version, client)?
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
