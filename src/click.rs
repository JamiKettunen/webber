use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use blake2::digest::{Input, VariableOutput};
use blake2::VarBlake2s;

use deunicode::deunicode;

const DESKTOP_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/999.9.9999.999 Safari/537.36";

#[derive(Debug)]
pub struct Package {
    pub url: String,
    pub name: String,
    pub theme_color: String,
    pub icon: Icon,
    pub url_patterns: String,
    pub permissions: Vec<String>,
    pub enable_address_bar: bool,
    pub enable_back_forward: bool,
    pub enable_fullscreen: bool,
    pub enable_desktop_user_agent: bool,
}

impl Package {
    pub fn create(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let path = xdg::BaseDirectories::new()?
            .get_cache_home()
            .join("webber.timsueberkrueb/click-build");
        fs::create_dir_all(&path)?;
        // Clean up
        fs::remove_dir_all(&path)?;
        fs::create_dir(&path)?;

        let control = path.join(Path::new("control"));
        let data = path.join(Path::new("data"));

        mkdir(&control)?;
        mkdir(&data)?;

        let click_binary = path.join(Path::new("click_binary"));
        let debian_binary = path.join(Path::new("debian-binary"));

        write_file(&click_binary, "0.4\n")?;
        write_file(&debian_binary, "2.0\n")?;
        write_file(
            &control.join(Path::new("control")),
            &control_control_content(&self.appname()),
        )?;

        let control_manifest = ControlManifest::new(self.appname(), self.name.clone());

        write_file(
            &control.join(Path::new("manifest")),
            &control_manifest.to_string()?,
        )?;
        write_file(&data.join(Path::new("preinst")), control_preinst_content())?;

        let apparmor = AppArmor::new(self.permissions.clone());

        // TODO: md5sums
        write_file(
            &data.join(Path::new("shortcut.apparmor")),
            &apparmor.to_string()?,
        )?;

        let icon_filename = match self.icon {
            Icon::Remote(ref icon_url) => {
                let ext = url::Url::parse(&icon_url)
                    .ok()
                    .map(|icon| Some(icon.path_segments()?.map(String::from).collect::<Vec<_>>()))
                    .map(|segments| segments?.iter().rev().cloned().next())
                    .map(|last| last?.rsplit('.').map(String::from).next())
                    .unwrap_or_default();
                if let Some(ext) = ext {
                    let icon_fname = format!("icon.{}", ext);
                    download_file(&icon_url, &data.join(Path::new(&icon_fname)))?;
                    Some(icon_fname)
                } else {
                    None
                }
            }
            Icon::Local(ref icon_path) => {
                if icon_path == "" {
                    None
                } else {
                    let ext = Path::new(&icon_path).extension();
                    let icon_fname = if let Some(ext) = ext {
                        format!("icon.{}", ext.to_str().unwrap())
                    } else {
                        "icon".to_owned()
                    };
                    std::fs::copy(icon_path, &data.join(Path::new(&icon_fname)))?;
                    Some(icon_fname)
                }
            }
        };

        let icon_filename = icon_filename.unwrap_or_else(|| {
            let icon_fname = "icon.svg".to_owned();
            write_icon(&data.join(Path::new(&icon_fname))).expect("Failed to write default icon");
            icon_fname
        });

        write_file(
            &data.join(Path::new("shortcut.desktop")),
            &data_desktop_content(&self, &icon_filename),
        )?;

        let control_tar_gz = path.join(Path::new("control.tar.gz"));
        let data_tar_gz = path.join(Path::new("data.tar.gz"));

        create_tar_gz(&control_tar_gz, &control)?;
        create_tar_gz(&data_tar_gz, &data)?;

        let click_path = path.join(Path::new(&format!("{}.click", self.package_name())));

        create_ar(
            &click_path,
            &[
                (&debian_binary, "debian-binary"),
                (&control_tar_gz, "control.tar.gz"),
                (&data_tar_gz, "data.tar.gz"),
                (&click_binary, "_click-binary"),
            ],
        )?;

        Ok(click_path)
    }

    fn package_name(&self) -> String {
        let stripped_name = deunicode(&self.name)
            .chars()
            .filter_map(|c|
                if ('a'..='z').contains(&c)
                    || ('A'..='Z').contains(&c)
                    || c.is_digit(10)
                    || c == ' ' || c == '-' || c == '.' {
                    Some(c)
                } else {
                    None
                }
            )
            .collect::<String>();
        if stripped_name.is_empty() {
            "Webapp".to_owned()
        } else {
            stripped_name
        }
    }

    fn appname(&self) -> String {
        let url = url::Url::parse(&self.url).ok();

        // Use the url host and a short hash of the path to generate a reasonably unique appname.
        // We use a short hash for the url path to ensure we don't exceed the name limits of unix
        // domain sockets (108 chars as stated here http://man7.org/linux/man-pages/man7/unix.7.html
        // for unix domain sockets). Note that the socket will be created by the webapp container
        // and the name will be of the format
        // "/home/phablet/.local/share/<host-part>-<path-hash>.webber/SingletonSocket"
        // Also note that we rely on the webapp container to use the short name format (see above).
        // The long format is
        // "/home/phablet/.local/share/<host-part>-<path-hash>.webber/<host-part>-<path-hash>/SingletonSocket".
        // UNIX_SOCKET_MAX_LEN is 107 characters because the string will become null-terminated.
        const UNIX_SOCKET_MAX_LEN: usize = 107;
        const SHORT_HASH_LEN: usize = 16;
        // 41 chars left for the encoded host name
        // FIXME: len is not yet stable as a const fn
        let available_len: usize = UNIX_SOCKET_MAX_LEN
            - "/home/phablet/.local/share/-.webber/SingletonSocket".len()
            - SHORT_HASH_LEN;

        let (url_host_part, url_path_part) = url
            .map(|url| {
                (
                    url.host_str().map(String::from).unwrap_or_default(),
                    url.path().to_owned(),
                )
            })
            .unwrap_or_default();

        let url_path_hash = if url_path_part != "/" && url_path_part != "" {
            // SHORT_HASH_LEN / 2 because we need (at most) two hex digits to encode a byte
            let mut short_hash =
                VarBlake2s::new(SHORT_HASH_LEN / 2).expect("Failed to create blake2 hasher");
            short_hash.input(url_path_part);
            hex::encode(short_hash.vec_result())
        } else {
            String::new()
        };

        // Remove forbidden characters
        let ascii = url_host_part.to_ascii_lowercase();
        let allowed_chars = ascii
            .chars()
            .filter_map(|c| {
                if c == '/' || c == '.' || c == '_' {
                    Some('-')
                } else if ('a'..='z').contains(&c) || c.is_digit(10) {
                    Some(c)
                } else {
                    None
                }
            })
            .collect::<String>();

        // Cut final string to allowed available len for host part
        let ascii_bytes = allowed_chars
            .into_bytes()
            .into_iter()
            .take(available_len)
            .collect::<Vec<_>>();

        // Note that this should always succeed since we ensure the string only contains a
        // restricted set of ASCII characters at this point.
        let final_host_part =
            String::from_utf8(ascii_bytes).expect("Failed to convert ascii bytes back to utf-8");

        if url_path_hash.is_empty() {
            final_host_part
        } else {
            format!("{}-{}", final_host_part, url_path_hash)
        }
    }
}

#[derive(Debug)]
pub enum Icon {
    Local(String),
    Remote(String),
}

#[derive(serde::Serialize)]
struct ControlManifest {
    architecture: String,
    description: String,
    framework: String,
    hooks: HashMap<String, ManifestHook>,
    maintainer: String,
    name: String,
    title: String,
    version: String,
}

impl ControlManifest {
    fn new(appname: String, title: String) -> Self {
        let mut hooks = HashMap::new();
        hooks.insert(
            appname.clone(),
            ManifestHook {
                apparmor: "shortcut.apparmor".to_owned(),
                desktop: "shortcut.desktop".to_owned(),
            },
        );
        Self {
            architecture: "all".to_owned(),
            description: "Shortcut".to_owned(),
            framework: "ubuntu-sdk-16.04".to_owned(),
            hooks,
            maintainer: "Webber <noreply@ubports.com>".to_owned(),
            name: format!("{}.webber", appname),
            title,
            version: "1.0.0".to_owned(),
        }
    }

    fn to_string(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

#[derive(serde::Serialize)]
struct ManifestHook {
    apparmor: String,
    desktop: String,
}

#[derive(serde::Serialize)]
struct AppArmor {
    template: String,
    policy_groups: Vec<String>,
    policy_version: String,
}

impl AppArmor {
    fn new(mut permissions: Vec<String>) -> Self {
        let mut policy_groups = vec!["networking".to_owned(), "webview".to_owned()];
        policy_groups.append(&mut permissions);
        Self {
            template: "ubuntu-webapp".to_owned(),
            policy_groups,
            policy_version: "16.04".to_owned(),
        }
    }

    fn to_string(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

fn download_file(url: &str, target: &Path) -> Result<(), Box<dyn Error>> {
    let mut resp = reqwest::get(url)?;
    let mut file = fs::File::create(target)?;
    io::copy(&mut resp, &mut file)?;
    Ok(())
}

fn create_ar(filepath: &Path, files: &[(&Path, &str)]) -> io::Result<()> {
    let file = fs::File::create(filepath)?;
    let mut archive = ar::Builder::new(file);
    for (src, target) in files {
        let mut file = fs::File::open(src)?;
        archive.append_file(&target.as_bytes(), &mut file)?;
    }
    Ok(())
}

fn create_tar_gz(filepath: &Path, dir: &Path) -> io::Result<()> {
    // FIXME: We cannot use the `tar` crate as for some reason the filepaths
    // need to start with ./ and this seem to get normalized away in Rust paths.
    // This workaround should be okay because we control the filepath, but it is ugly
    // nevertheless.
    Command::new(
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("tar"),
    )
    .args(&[
        "--transform",
        &format!(
            "flags=r;s|{}|.|",
            dir.file_name().unwrap().to_str().unwrap()
        ),
        "-czf",
        filepath.to_str().unwrap(),
        dir.file_name().unwrap().to_str().unwrap(),
    ])
    .current_dir(&dir.join(".."))
    .output()?;
    Ok(())
}

fn mkdir(dirname: &Path) -> io::Result<()> {
    fs::create_dir(dirname)
}

fn write_file(filename: &Path, content: &str) -> io::Result<()> {
    let mut file = fs::File::create(filename)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

fn control_control_content(appname: &str) -> String {
    format!(
        r#"Package: {}.webber
Version: 1.0.0
Click-Version: 0.4
Architecture: all
Maintainer: Webber <noreply@ubports.com>
Description: Shortcut
"#,
        appname,
    )
}

fn control_preinst_content() -> &'static str {
    r#"#! /bin/sh
echo "Click packages may not be installed directly using dpkg."
echo "Use 'click install' instead."
exit 1"#
}

fn data_desktop_content(package: &Package, icon_fname: &str) -> String {
    let mut optional_flags = Vec::new();
    if package.enable_address_bar {
        optional_flags.push("--enable-addressbar");
    }
    if package.enable_back_forward {
        optional_flags.push("--enable-back-forward");
    }
    if package.enable_fullscreen {
        optional_flags.push("--fullscreen");
    }
    let ua_flag = format!("--user-agent-string={}", DESKTOP_USER_AGENT);
    if package.enable_desktop_user_agent {
        optional_flags.push(&ua_flag);
    }
    optional_flags.push(&package.url);
    let flags_and_url = optional_flags.join(" ");
    format!(
        r#"[Desktop Entry]
Name={}
Exec=webapp-container --webappUrlPatterns={} --store-session-cookies --enable-media-hub-audio {}
Icon={}
Terminal=false
Type=Application
X-Ubuntu-Touch=true
X-Ubuntu-Splash-Color={}
"#,
        package.name, package.url_patterns, flags_and_url, icon_fname, package.theme_color
    )
}

fn write_icon(path: &Path) -> io::Result<()> {
    let bytes = include_bytes!("../assets/logo.svg");
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    Ok(())
}
