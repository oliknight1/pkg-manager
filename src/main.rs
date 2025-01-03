use flate2::read::GzDecoder;
use reqwest::{blocking::Client, Error};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::path::{self, Path};
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

fn main() {
    let client = Client::new();
    let json = fs::read_to_string("./package.json").expect("Error reading file");

    let json: PackageJSON = serde_json::from_str(&json).expect("Error reading json");

    println!("{:#?}", json);

    match json.dependencies {
        Some(deps) => {
            println!("depppps {:?}", deps);

            if Path::new("dep-lock.json").exists() {
                if let Err(e) = fetch_dep_from_lock(&client) {
                    eprintln!("Error: {e}")
                };
            } else {
                if let Err(e) = fetch_dep(deps, &client) {
                    eprintln!("Error: {e}")
                }
            }
        }
        None => println!("No dependencies"),
    }
}

/// fetch the dependency using the url from lock file
fn fetch_dep_from_lock(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let lock = fs::read_to_string("dep-lock.json")?;
    let lock: LockFile = serde_json::from_str(&lock)?;
    for (key, value) in lock.iter() {
        let url = value.resolved_url.clone();
        if let Err(e) = fetch_tarball(url, key.to_string(), client) {
            return Err(format!("Error fetching tarball: {e}").into());
        }
    }
    Ok(())
}

fn fetch_dep(
    dependencies: HashMap<String, String>,
    client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
    for (name, version) in dependencies {
        let matched_version = get_latest_version_name(&name, &version, &client);
        match matched_version {
            Ok(mv) => {
                println!("matched version: {:?}", mv.dist.tarball);
                if let Err(e) = fetch_tarball(mv.dist.tarball, name, client) {
                    return Err(format!("Error fetching tarball: {e}").into());
                }
            }
            Err(e) => {
                println!("Error: {e}")
            }
        }
    }
    Ok(())
}

fn fetch_tarball(
    url: String,
    name: String,
    client: &Client,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = client.get(&url).send()?;

    let content = Cursor::new(response.bytes()?);

    let tar = GzDecoder::new(content);
    let mut archive = Archive::new(tar);
    let output_dir = format!("./node_modules/{name}");

    //TODO: need to make sure each dep removes the root directory
    archive.unpack(output_dir)?;
    Ok(())
}

///  Parses the dependency version parsed from package.json. If the dependency is a range, find the latest version
///
/// * `name`: name of the dependency
/// * `version`: version specified in the package.json
/// * `client`: reqwest client
fn get_latest_version_name(
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
