use anyhow::{Context, Result, bail, ensure};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use std::{
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

const SQLITE: &[&str] = &["db", "sqlite", "sqlite3"];
const XLSX: &[&str] = &["xlsx"];
fn extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .is_some_and(|ext| allowed.iter().any(|x| ext.eq_ignore_ascii_case(x)))
}

pub struct Workspace {
    pub root: PathBuf,
    dir: Dir,
}
pub struct SqliteFile {
    dir: Dir,
    pub path: PathBuf,
}
impl SqliteFile {
    pub fn validate(&self, name: &str, may_create: bool) -> Result<()> {
        for (file, optional) in [
            (name.to_owned(), may_create),
            (format!("{name}-journal"), true),
            (format!("{name}-wal"), true),
            (format!("{name}-shm"), true),
        ] {
            match self.dir.symlink_metadata(&file) {
                Ok(meta) => {
                    ensure!(
                        meta.is_file(),
                        "SQLite database and sidecars must be regular files, not symlinks"
                    );
                    single_link(&meta)?;
                }
                Err(err) if optional && err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err).context("open SQLite workspace file"),
            }
        }
        Ok(())
    }
}
#[cfg(unix)]
fn single_link(meta: &cap_std::fs::Metadata) -> Result<()> {
    use cap_std::fs::MetadataExt;
    ensure!(
        meta.nlink() == 1,
        "hard-linked workspace files are not supported"
    );
    Ok(())
}
#[cfg(not(unix))]
fn single_link(_: &cap_std::fs::Metadata) -> Result<()> {
    Ok(())
}

impl Workspace {
    pub fn open(root: &str) -> Result<Self> {
        ensure!(
            Path::new(root).is_absolute(),
            "workspace_root must be absolute"
        );
        let root = std::fs::canonicalize(root).context("workspace_root is unavailable")?;
        let dir = Dir::open_ambient_dir(&root, ambient_authority())?;
        Ok(Self { root, dir })
    }
    fn resolve(&self, input: &str, allowed: &[&str]) -> Result<(Dir, String, PathBuf)> {
        ensure!(!input.is_empty() && !input.contains('\0'), "invalid workspace path");
        #[cfg(not(windows))]
        ensure!(!input.contains(['\\', ':']), "invalid workspace path");
        #[cfg(windows)]
        let cleaned_input = input
            .strip_prefix(r"\\?\")
            .or_else(|| input.strip_prefix("//?/"))
            .unwrap_or(input)
            .replace('\\', "/");
        #[cfg(not(windows))]
        let cleaned_input = input.to_owned();
        let p = Path::new(&cleaned_input);
        let p_owned: PathBuf;
        let p = if p.is_absolute() {
            if let Ok(rel) = p.strip_prefix(&self.root) {
                rel
            } else {
                let root_str = self.root.to_string_lossy();
                let clean_root = root_str
                    .strip_prefix(r"\\?\")
                    .or_else(|| root_str.strip_prefix("//?/"))
                    .unwrap_or(&root_str)
                    .replace('\\', "/");
                #[cfg(windows)]
                let stripped = cleaned_input
                    .to_ascii_lowercase()
                    .strip_prefix(&clean_root.to_ascii_lowercase())
                    .map(|_| cleaned_input[clean_root.len()..].trim_start_matches('/'));
                #[cfg(not(windows))]
                let stripped = cleaned_input
                    .strip_prefix(&clean_root)
                    .map(|rest| rest.trim_start_matches('/'));
                let rel = stripped.context("path is outside workspace")?;
                p_owned = PathBuf::from(rel);
                p_owned.as_path()
            }
        } else {
            #[cfg(windows)]
            ensure!(!cleaned_input.contains(':'), "invalid workspace path");
            p
        };
        ensure!(extension(p, allowed), "unexpected workspace file extension");
        let mut parts = p.components().peekable();
        let mut dir = self.dir.try_clone()?;
        let mut canonical = self.root.clone();
        while let Some(part) = parts.next() {
            match part {
                Component::Normal(name) if parts.peek().is_some() => {
                    dir = dir
                        .open_dir_nofollow(name)
                        .context("workspace directory missing or symlink")?;
                    canonical.push(name);
                }
                Component::Normal(name) => {
                    canonical.push(name);
                    return Ok((
                        dir,
                        name.to_str()
                            .context("workspace path must be UTF-8")?
                            .into(),
                        canonical,
                    ));
                }
                _ => bail!("path must stay within workspace"),
            }
        }
        bail!("missing workspace file name")
    }
    pub fn sqlite(&self, path: &str, write: bool) -> Result<SqliteFile> {
        let (dir, name, path) = self.resolve(path, SQLITE)?;
        // SQLite opens companion files itself. Reject symlinks and hard links for all of them.
        // SQLite's Unix VFS also uses O_NOFOLLOW when opening ordinary database files.
        let file = SqliteFile { dir, path };
        ensure!(
            std::fs::canonicalize(file.path.parent().context("no database parent")?)?
                == file.path.parent().unwrap(),
            "SQLite parent changed"
        );
        file.validate(&name, write)?;
        Ok(file)
    }
    pub fn discover(&self) -> Result<Vec<String>> {
        let mut files = Vec::new();
        for entry in self.dir.entries()?.take(10_000) {
            let entry = entry?;
            if let Some(name) = entry
                .file_name()
                .to_str()
                .filter(|name| !name.starts_with('.') && extension(Path::new(name), SQLITE))
                && entry.file_type()?.is_file()
                && self.sqlite(name, false).is_ok()
            {
                files.push(name.to_owned());
            }
        }
        files.sort_unstable();
        Ok(files)
    }
    pub fn read_xlsx(&self, path: &str) -> Result<Vec<u8>> {
        let (dir, name, _) = self.resolve(path, XLSX)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        ensure!(
            dir.symlink_metadata(&name)?.is_file(),
            "expected a regular workbook file"
        );
        let file = dir.open_with(name, &options)?;
        let meta = file.metadata()?;
        ensure!(meta.is_file(), "expected a regular workbook file");
        single_link(&meta)?;
        let mut bytes = Vec::new();
        file.take(excel_editor_engine::codec::MAX_FILE as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    pub fn write_xlsx(&self, path: &str, bytes: &[u8], overwrite: bool) -> Result<()> {
        let (dir, name, _) = self.resolve(path, XLSX)?;
        if let Ok(meta) = dir.symlink_metadata(&name) {
            ensure!(
                overwrite && meta.is_file(),
                "target exists or is not a regular workbook"
            );
            single_link(&meta)?;
        }
        let tmp = format!(".db-export-{}.tmp", uuid::Uuid::new_v4());
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let mut file = dir.open_with(&tmp, &options)?;
        let result = (|| -> Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            if overwrite {
                dir.rename(&tmp, &dir, &name)?;
            } else {
                dir.hard_link(&tmp, &dir, &name)?;
            }
            Ok(())
        })();
        let _ = dir.remove_file(&tmp);
        result
    }
}
