use anyhow::{Context, Result, ensure};
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Sqlite,
    Mysql,
    Postgres,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Dev,
    Staging,
    Prod,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub driver: Kind,
    pub env: Environment,
    pub read_only: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    pub ssl_mode: Option<String>,
    pub file_path: Option<String>,
    pub workspace_roots: Vec<PathBuf>,
    pub last_used_at: Option<String>,
    pub notes: String,
}
impl Profile {
    pub fn public(&self) -> serde_json::Value {
        let mut copy = self.clone();
        let has_password = copy.password.take().is_some();
        let mut value = serde_json::to_value(copy).expect("connection profile is serializable");
        value["has_password"] = has_password.into();
        value
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 256,
            "connection name is required (max 256 bytes)"
        );
        ensure!(
            self.id.len() <= 128
                && self
                    .id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
            "invalid connection id"
        );
        ensure!(self.notes.len() <= 4096, "notes exceed 4 KiB");
        if self.driver == Kind::Sqlite {
            ensure!(
                self.file_path.as_ref().is_some_and(|p| !p.is_empty()),
                "file_path is required for SQLite"
            );
        } else {
            for item in [&self.host, &self.database, &self.username] {
                ensure!(
                    item.as_ref()
                        .is_some_and(|s| !s.trim().is_empty() && s.len() <= 1024),
                    "host, database and username are required"
                );
            }
            ensure!(self.port != Some(0), "invalid port");
            ensure!(
                matches!(
                    self.ssl_mode.as_deref(),
                    None | Some(
                        "disabled" | "preferred" | "required" | "verify_ca" | "verify_full"
                    )
                ),
                "invalid ssl_mode"
            );
        }
        Ok(())
    }
    pub fn require_write(&self, confirmed: bool) -> Result<()> {
        ensure!(!self.read_only, "connection is read_only");
        ensure!(
            self.env != Environment::Prod || confirmed,
            "prod write requires confirm_prod: true"
        );
        Ok(())
    }
    pub fn clean_error(&self, err: anyhow::Error) -> anyhow::Error {
        let mut message = format!("{err:#}");
        if let Some(secret) = self.password.as_ref().filter(|s| !s.is_empty()) {
            message = message.replace(secret, "[redacted]");
        }
        anyhow::anyhow!(message)
    }
}

#[derive(Deserialize, Serialize)]
struct Profiles {
    version: u8,
    connections: Vec<Profile>,
}
#[derive(Clone)]
pub struct Config {
    dir: PathBuf,
}
impl Config {
    pub fn from_env() -> Result<Self> {
        let dir = if let Some(p) = std::env::var_os("DB_STUDIO_DATA_DIR") {
            PathBuf::from(p)
        } else {
            PathBuf::from(std::env::var_os("HOME").context("HOME is required")?)
                .join(".config/grow-db")
        };
        ensure!(dir.is_absolute(), "DB_STUDIO_DATA_DIR must be absolute");
        Ok(Self { dir })
    }
    fn access<T>(
        &self,
        root: &Path,
        change: impl FnOnce(&mut Vec<Profile>) -> Result<(T, bool)>,
    ) -> Result<T> {
        // Resolve the first existing ancestor before creating a directory so an override cannot put credentials in the project.
        let normalize = || -> Result<PathBuf> {
            let mut probe = self.dir.as_path();
            let mut suffix = Vec::new();
            while !probe.exists() {
                suffix.push(probe.file_name().context("invalid credentials directory")?);
                probe = probe.parent().context("invalid credentials directory")?;
            }
            let mut resolved = fs::canonicalize(probe)?;
            for name in suffix.into_iter().rev() {
                resolved.push(name);
            }
            ensure!(
                !resolved.starts_with(root)
                    && !resolved.ancestors().any(|p| p.join(".git").exists()),
                "credential storage cannot be inside a project repository"
            );
            Ok(resolved)
        };
        let dir = normalize()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir)?;
        }
        #[cfg(not(unix))]
        fs::create_dir_all(&dir)?;
        ensure!(normalize()? == dir, "credentials directory changed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                fs::metadata(&dir)?.permissions().mode() & 0o077 == 0,
                "credential directory must be private (0700)"
            );
        }
        let local = cap_std::fs::Dir::open_ambient_dir(&dir, cap_std::ambient_authority())?;
        let path = dir.join("connections.json");
        use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
        let mut lock_opts = cap_std::fs::OpenOptions::new();
        lock_opts
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            lock_opts.mode(0o600);
        }
        let lock = local.open_with(".lock", &lock_opts)?.into_std();
        FileExt::lock_exclusive(&lock)?;
        let mut read_opts = cap_std::fs::OpenOptions::new();
        read_opts.read(true).follow(FollowSymlinks::No);
        let mut data = match local.open_with("connections.json", &read_opts) {
            Ok(file) => {
                let meta = file.metadata()?;
                ensure!(
                    meta.is_file() && meta.len() <= 4 * 1024 * 1024,
                    "invalid connection store"
                );
                #[cfg(unix)]
                {
                    use cap_std::fs::{MetadataExt, PermissionsExt};
                    ensure!(
                        meta.nlink() == 1 && meta.permissions().mode() & 0o777 == 0o600,
                        "connections.json must have mode 0600 and a single link"
                    );
                }
                serde_json::from_reader(file.take(4 * 1024 * 1024 + 1))
                    .map_err(|_| anyhow::anyhow!("invalid connection store JSON"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Profiles {
                version: 1,
                connections: vec![],
            },
            Err(e) => return Err(e).context("cannot open connection store"),
        };
        ensure!(data.version == 1, "unsupported connection store version");
        let (value, dirty) = change(&mut data.connections)?;
        if dirty {
            let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                temp.as_file()
                    .set_permissions(fs::Permissions::from_mode(0o600))?;
            }
            serde_json::to_writer_pretty(&mut temp, &data)?;
            temp.write_all(b"\n")?;
            temp.as_file().sync_all()?;
            temp.persist(path)?;
        }
        Ok(value)
    }
    pub fn list(&self, root: &Path) -> Result<Vec<Profile>> {
        self.access(root, |profiles| {
            let mut profiles = profiles.clone();
            profiles.sort_by(|a, b| {
                b.workspace_roots
                    .contains(&root.to_path_buf())
                    .cmp(&a.workspace_roots.contains(&root.to_path_buf()))
                    .then_with(|| b.last_used_at.cmp(&a.last_used_at))
                    .then_with(|| a.name.cmp(&b.name))
            });
            Ok((profiles, false))
        })
    }
    pub fn get(&self, root: &Path, id: &str) -> Result<Profile> {
        self.access(root, |profiles| {
            Ok((
                profiles
                    .iter()
                    .find(|p| p.id == id)
                    .context("connection not found")?
                    .clone(),
                false,
            ))
        })
    }
    pub fn prepare(&self, root: &Path, mut profile: Profile) -> Result<Profile> {
        profile.validate()?;
        profile.workspace_roots.clear();
        profile.last_used_at = None;
        if profile.id.is_empty() {
            profile.id = uuid::Uuid::new_v4().to_string();
        } else if let Some(prev) = self.list(root)?.into_iter().find(|p| p.id == profile.id) {
            if profile.password.as_deref().is_none_or(str::is_empty)
                && profile.driver == prev.driver
            {
                profile.password = prev.password;
            }
        }
        if profile.driver == Kind::Sqlite {
            profile.password = None;
        } else {
            profile.password = profile.password.filter(|p| !p.is_empty());
        }
        Ok(profile)
    }
    pub fn save(&self, root: &Path, mut profile: Profile) -> Result<Profile> {
        self.access(root, |profiles| {
            if let Some(prev) = profiles.iter_mut().find(|p| p.id == profile.id) {
                profile.workspace_roots = prev.workspace_roots.clone();
                mark(&mut profile, root);
                *prev = profile.clone();
            } else {
                ensure!(profiles.len() < 1000, "connection store is full");
                mark(&mut profile, root);
                profiles.push(profile.clone());
            }
            Ok((profile, true))
        })
    }
    pub fn touch(&self, root: &Path, id: &str) -> Result<()> {
        self.access(root, |profiles| {
            let found = profiles.iter_mut().find(|p| p.id == id);
            if let Some(p) = found {
                mark(p, root);
                Ok(((), true))
            } else {
                Ok(((), false))
            }
        })
    }
    pub fn delete(&self, root: &Path, id: &str) -> Result<()> {
        self.access(root, |profiles| {
            let old = profiles.len();
            profiles.retain(|p| p.id != id);
            ensure!(old != profiles.len(), "connection not found");
            Ok(((), true))
        })
    }
}
fn mark(profile: &mut Profile, root: &Path) {
    if !profile.workspace_roots.iter().any(|p| p == root) {
        if profile.workspace_roots.len() >= 64 {
            profile.workspace_roots.remove(0);
        }
        profile.workspace_roots.push(root.to_owned());
    }
    profile.last_used_at = Some(Utc::now().to_rfc3339());
}
