//! Pure application core for the local-first project model.
//!
//! This crate deliberately contains no filesystem, network, Tauri, or UI code.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::fmt;

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROJECT_NAME_CHARS: usize = 120;
pub const MAX_FILE_NAME_CHARS: usize = 180;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectCoreError {
    InvalidId { kind: &'static str, value: String },
    InvalidName(String),
    InvalidPath(String),
    InvalidTimestamp(String),
    InvalidContentType(String),
    InvalidDigest(String),
    InvalidCreation(String),
    DuplicateMaterial(MaterialId),
    DuplicateCreation(CreationId),
    MissingMaterial(MaterialId),
    MissingCreation(CreationId),
    NotFound(ProjectId),
    AlreadyExists(ProjectId),
    Conflict { project_id: ProjectId },
    CorruptMetadata(String),
    UnsupportedSchema(u32),
    StorageUnavailable,
    AtomicWriteFailed,
    SourceUnreadable,
    PathEscape,
    SymlinkRejected,
    WriteFailed,
    IntegrityMismatch,
    OperationFailed { operation: &'static str },
}
impl fmt::Display for ProjectCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ProjectCoreError::*;
        match self {
            InvalidId { kind, .. } => write!(f, "invalid {kind}"),
            InvalidName(_) => f.write_str("invalid project name"),
            InvalidPath(_) => f.write_str("invalid project-relative path"),
            InvalidTimestamp(_) => f.write_str("invalid timestamp"),
            InvalidContentType(_) => f.write_str("invalid content type"),
            InvalidDigest(_) => f.write_str("invalid SHA-256 digest"),
            InvalidCreation(_) => f.write_str("invalid creation"),
            DuplicateMaterial(_) => f.write_str("duplicate material"),
            DuplicateCreation(_) => f.write_str("duplicate creation"),
            MissingMaterial(_) => f.write_str("material not found"),
            MissingCreation(_) => f.write_str("creation not found"),
            NotFound(_) => f.write_str("project not found"),
            AlreadyExists(_) => f.write_str("project already exists"),
            Conflict { .. } => f.write_str("project was changed elsewhere"),
            CorruptMetadata(_) => f.write_str("corrupt project metadata"),
            UnsupportedSchema(_) => f.write_str("unsupported project schema"),
            StorageUnavailable => f.write_str("project storage is unavailable"),
            AtomicWriteFailed => f.write_str("could not safely save project metadata"),
            SourceUnreadable => f.write_str("material source could not be read"),
            PathEscape => f.write_str("project path escapes its fixed root"),
            SymlinkRejected => f.write_str("symbolic links are not allowed"),
            WriteFailed => f.write_str("project content could not be saved"),
            IntegrityMismatch => f.write_str("stored project content failed verification"),
            OperationFailed { .. } => f.write_str("project operation failed"),
        }
    }
}
impl std::error::Error for ProjectCoreError {}
pub type CoreResult<T> = Result<T, ProjectCoreError>;

macro_rules! uuid_id {
    ($name:ident, $kind:literal) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
                let value = value.into();
                if is_uuid_v7(&value) {
                    Ok(Self(value))
                } else {
                    Err(ProjectCoreError::InvalidId { kind: $kind, value })
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}
uuid_id!(ProjectId, "project ID");
uuid_id!(MaterialId, "material ID");
uuid_id!(CreationId, "creation ID");
fn is_uuid_v7(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 36
        && [8, 13, 18, 23].iter().all(|&i| b[i] == b'-')
        && b[14] == b'7'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
        && b.iter().enumerate().all(|(i, c)| {
            [8, 13, 18, 23].contains(&i) || c.is_ascii_hexdigit() && !c.is_ascii_uppercase()
        })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(String);
impl Timestamp {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if is_rfc3339_utc_second(&value) {
            Ok(Self(value))
        } else {
            Err(ProjectCoreError::InvalidTimestamp(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
fn is_rfc3339_utc_second(v: &str) -> bool {
    let b = v.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
        || ![0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
    {
        return false;
    }
    let n = |a, b| v.get(a..b).and_then(|s| s.parse::<u32>().ok());
    let (Some(y), Some(m), Some(d), Some(h), Some(mi), Some(s)) =
        (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19))
    else {
        return false;
    };
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 400 == 0 || y % 4 == 0 && y % 100 != 0 => 29,
        2 => 28,
        _ => return false,
    };
    d >= 1 && d <= dim && h < 24 && mi < 60 && s < 60
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectName(String);
impl ProjectName {
    pub fn parse(value: impl AsRef<str>) -> CoreResult<Self> {
        let t = value.as_ref().trim();
        if t.is_empty()
            || t.chars().count() > MAX_PROJECT_NAME_CHARS
            || t.contains(['/', '\\', '\0'])
        {
            Err(ProjectCoreError::InvalidName(value.as_ref().into()))
        } else {
            Ok(Self(t.into()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativeProjectPath(String);
impl RelativeProjectPath {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.starts_with('/')
            || value.contains('\\')
            || value
                .split('/')
                .any(|s| s.is_empty() || matches!(s, "." | ".."))
        {
            Err(ProjectCoreError::InvalidPath(value))
        } else {
            Ok(Self(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn starts_with_root(&self, root: &str) -> bool {
        self.0
            .strip_prefix(root)
            .is_some_and(|r| r.starts_with('/'))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentType(String);
impl ContentType {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 255
            || value
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            || !value.contains('/')
        {
            Err(ProjectCoreError::InvalidContentType(value))
        } else {
            Ok(Self(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sha256Digest(String);
impl Sha256Digest {
    pub fn parse(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            Ok(Self(value))
        } else {
            Err(ProjectCoreError::InvalidDigest(value))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectState {
    Local,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CreationKind {
    Web,
    Document,
    Image,
    File,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Material {
    pub id: MaterialId,
    pub display_name: String,
    pub original_file_name: String,
    pub relative_path: RelativeProjectPath,
    pub content_type: Option<ContentType>,
    pub byte_size: u64,
    pub sha256: Sha256Digest,
    pub created_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Creation {
    pub id: CreationId,
    pub display_name: String,
    pub kind: CreationKind,
    pub relative_path: RelativeProjectPath,
    pub content_type: Option<ContentType>,
    pub byte_size: u64,
    pub revision: u32,
    pub parent_creation_id: Option<CreationId>,
    pub created_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub schema_version: u32,
    pub id: ProjectId,
    pub name: ProjectName,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub state: ProjectState,
    pub materials: Vec<Material>,
    pub creations: Vec<Creation>,
}
impl Project {
    pub fn new(id: ProjectId, name: ProjectName, now: Timestamp) -> Self {
        Self {
            schema_version: PROJECT_SCHEMA_VERSION,
            id,
            name,
            created_at: now.clone(),
            updated_at: now,
            state: ProjectState::Local,
            materials: vec![],
            creations: vec![],
        }
    }
    pub fn validate(&self) -> CoreResult<()> {
        if self.schema_version != PROJECT_SCHEMA_VERSION {
            return Err(ProjectCoreError::UnsupportedSchema(self.schema_version));
        }
        let mut ms = HashSet::new();
        for m in &self.materials {
            if !ms.insert(m.id.clone()) {
                return Err(ProjectCoreError::DuplicateMaterial(m.id.clone()));
            }
            validate_file_metadata(
                &m.display_name,
                &m.original_file_name,
                &m.relative_path,
                "inputs",
                m.id.as_str(),
            )?
        }
        let mut cs = HashSet::new();
        for c in &self.creations {
            if !cs.insert(c.id.clone()) {
                return Err(ProjectCoreError::DuplicateCreation(c.id.clone()));
            }
            if c.display_name.trim().is_empty() || c.revision != 1 {
                return Err(ProjectCoreError::InvalidCreation(c.display_name.clone()));
            }
            validate_file_metadata(
                &c.display_name,
                &c.display_name,
                &c.relative_path,
                "outputs",
                c.id.as_str(),
            )?
        }
        for c in &self.creations {
            if let Some(parent) = &c.parent_creation_id
                && (parent == &c.id || !cs.contains(parent))
            {
                return Err(ProjectCoreError::InvalidCreation(
                    "parent creation is absent".into(),
                ));
            }
        }
        Ok(())
    }
}
fn validate_file_metadata(
    display: &str,
    file: &str,
    path: &RelativeProjectPath,
    root: &str,
    id: &str,
) -> CoreResult<()> {
    if display.trim().is_empty()
        || file.trim().is_empty()
        || file.contains(['/', '\\', '\0'])
        || !path.starts_with_root(root)
    {
        return Err(ProjectCoreError::InvalidPath(path.as_str().into()));
    }
    if !path.as_str().starts_with(&format!("{root}/{id}/")) {
        return Err(ProjectCoreError::PathEscape);
    }
    Ok(())
}

pub trait Clock {
    fn now(&self) -> Timestamp;
}
pub trait IdGenerator {
    fn project_id(&self) -> ProjectId;
    fn material_id(&self) -> MaterialId;
    fn creation_id(&self) -> CreationId;
}
pub trait ProjectRepository {
    fn create(&mut self, project: &Project) -> CoreResult<()>;
    fn get(&self, id: &ProjectId) -> CoreResult<Project>;
    fn list(&self) -> CoreResult<Vec<Project>>;
    fn replace(&mut self, project: &Project, expected_updated_at: &Timestamp) -> CoreResult<()>;
    fn delete(&mut self, id: &ProjectId) -> CoreResult<()>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialContent {
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreationContent {
    pub bytes: Vec<u8>,
    pub file_name: String,
}
/// A descriptor returned only after an adapter has copied and SHA-256 hashed a material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredMaterial {
    pub relative_path: RelativeProjectPath,
    pub byte_size: u64,
    pub sha256: Sha256Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredCreation {
    pub relative_path: RelativeProjectPath,
    pub byte_size: u64,
}
pub trait ProjectContentStore {
    fn store_material(
        &mut self,
        p: &ProjectId,
        m: &MaterialId,
        source: &MaterialContent,
        safe_file_name: &str,
    ) -> CoreResult<StoredMaterial>;
    fn read_material(&self, p: &ProjectId, m: &Material) -> CoreResult<Vec<u8>>;
    fn store_creation(
        &mut self,
        p: &ProjectId,
        c: &CreationId,
        content: &CreationContent,
        safe_file_name: &str,
    ) -> CoreResult<StoredCreation>;
    fn read_creation(&self, p: &ProjectId, c: &Creation) -> CoreResult<Vec<u8>>;
    fn remove_project_tree(&mut self, p: &ProjectId) -> CoreResult<()>;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddMaterial {
    pub display_name: String,
    pub original_file_name: String,
    pub content_type: Option<ContentType>,
    pub source: MaterialContent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateCreation {
    pub display_name: String,
    pub kind: CreationKind,
    pub content_type: Option<ContentType>,
    pub content: CreationContent,
    pub parent_creation_id: Option<CreationId>,
}
pub struct ProjectService<R, S, C, I> {
    repository: R,
    content: S,
    clock: C,
    ids: I,
}
impl<R, S, C, I> ProjectService<R, S, C, I>
where
    R: ProjectRepository,
    S: ProjectContentStore,
    C: Clock,
    I: IdGenerator,
{
    pub fn new(repository: R, content: S, clock: C, ids: I) -> Self {
        Self {
            repository,
            content,
            clock,
            ids,
        }
    }
    #[cfg(test)]
    fn into_parts(self) -> (R, S, C, I) {
        (self.repository, self.content, self.clock, self.ids)
    }
    pub fn create_project(&mut self, name: impl AsRef<str>) -> CoreResult<Project> {
        let p = Project::new(
            self.ids.project_id(),
            ProjectName::parse(name)?,
            self.clock.now(),
        );
        self.repository.create(&p)?;
        Ok(p)
    }
    pub fn open_project(&self, id: &ProjectId) -> CoreResult<Project> {
        self.repository.get(id)
    }
    pub fn list_projects(&self) -> CoreResult<Vec<Project>> {
        self.repository.list()
    }
    pub fn rename_project(&mut self, id: &ProjectId, name: impl AsRef<str>) -> CoreResult<Project> {
        let mut p = self.repository.get(id)?;
        let e = p.updated_at.clone();
        p.name = ProjectName::parse(name)?;
        p.updated_at = self.clock.now();
        self.repository.replace(&p, &e)?;
        Ok(p)
    }
    pub fn delete_project(&mut self, id: &ProjectId) -> CoreResult<()> {
        self.repository.get(id)?;
        self.repository.delete(id)?;
        self.content.remove_project_tree(id)
    }
    pub fn add_material(&mut self, pid: &ProjectId, r: AddMaterial) -> CoreResult<Material> {
        if r.display_name.trim().is_empty() || r.original_file_name.trim().is_empty() {
            return Err(ProjectCoreError::InvalidName(r.display_name));
        }
        let mut p = self.repository.get(pid)?;
        let id = self.ids.material_id();
        if p.materials.iter().any(|m| m.id == id) {
            return Err(ProjectCoreError::DuplicateMaterial(id));
        }
        let s = safe_file_name(&r.original_file_name);
        let stored = self.content.store_material(pid, &id, &r.source, &s)?;
        ensure_stored_path(&stored.relative_path, "inputs", id.as_str())?;
        let m = Material {
            id,
            display_name: r.display_name,
            original_file_name: r.original_file_name,
            relative_path: stored.relative_path,
            content_type: r.content_type,
            byte_size: stored.byte_size,
            sha256: stored.sha256,
            created_at: self.clock.now(),
        };
        let e = p.updated_at.clone();
        p.materials.push(m.clone());
        p.updated_at = m.created_at.clone();
        self.repository.replace(&p, &e)?;
        Ok(m)
    }
    pub fn read_material(&self, pid: &ProjectId, id: &MaterialId) -> CoreResult<Vec<u8>> {
        let p = self.repository.get(pid)?;
        let m = p
            .materials
            .iter()
            .find(|m| &m.id == id)
            .ok_or_else(|| ProjectCoreError::MissingMaterial(id.clone()))?;
        self.content.read_material(pid, m)
    }
    pub fn create_creation(&mut self, pid: &ProjectId, r: CreateCreation) -> CoreResult<Creation> {
        if r.display_name.trim().is_empty() || r.content.file_name.trim().is_empty() {
            return Err(ProjectCoreError::InvalidCreation(r.display_name));
        }
        let mut p = self.repository.get(pid)?;
        if let Some(parent) = &r.parent_creation_id
            && !p.creations.iter().any(|c| &c.id == parent)
        {
            return Err(ProjectCoreError::InvalidCreation(
                "parent creation is absent".into(),
            ));
        }
        let id = self.ids.creation_id();
        if p.creations.iter().any(|c| c.id == id) {
            return Err(ProjectCoreError::DuplicateCreation(id));
        }
        let s = safe_file_name(&r.content.file_name);
        let stored = self.content.store_creation(pid, &id, &r.content, &s)?;
        ensure_stored_path(&stored.relative_path, "outputs", id.as_str())?;
        let c = Creation {
            id,
            display_name: r.display_name,
            kind: r.kind,
            relative_path: stored.relative_path,
            content_type: r.content_type,
            byte_size: stored.byte_size,
            revision: 1,
            parent_creation_id: r.parent_creation_id,
            created_at: self.clock.now(),
        };
        let e = p.updated_at.clone();
        p.creations.push(c.clone());
        p.updated_at = c.created_at.clone();
        self.repository.replace(&p, &e)?;
        Ok(c)
    }
    pub fn list_creations(&self, pid: &ProjectId) -> CoreResult<Vec<Creation>> {
        Ok(self.repository.get(pid)?.creations)
    }
    pub fn read_creation(&self, pid: &ProjectId, id: &CreationId) -> CoreResult<Vec<u8>> {
        let p = self.repository.get(pid)?;
        let c = p
            .creations
            .iter()
            .find(|c| &c.id == id)
            .ok_or_else(|| ProjectCoreError::MissingCreation(id.clone()))?;
        self.content.read_creation(pid, c)
    }
}
fn ensure_stored_path(path: &RelativeProjectPath, root: &str, id: &str) -> CoreResult<()> {
    validate_file_metadata("stored content", "stored-content", path, root, id)
}
pub fn safe_file_name(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars().take(MAX_FILE_NAME_CHARS) {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            dash = false
        } else if !dash {
            out.push('-');
            dash = true
        }
    }
    let out = out.trim_matches(['.', '-']).to_owned();
    if out.is_empty() { "file".into() } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    const P: &str = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
    const M: &str = "0198e4a6-79b2-7b51-9e68-c2eb7af3db14";
    const C: &str = "0198e4a6-86d6-7c16-b4c4-3197b355cf10";
    fn pid() -> ProjectId {
        ProjectId::parse(P).unwrap()
    }
    fn time() -> Timestamp {
        Timestamp::parse("2026-08-28T15:00:00Z").unwrap()
    }
    #[derive(Clone)]
    struct TestClock;
    impl Clock for TestClock {
        fn now(&self) -> Timestamp {
            time()
        }
    }
    struct TestIds;
    impl IdGenerator for TestIds {
        fn project_id(&self) -> ProjectId {
            pid()
        }
        fn material_id(&self) -> MaterialId {
            MaterialId::parse(M).unwrap()
        }
        fn creation_id(&self) -> CreationId {
            CreationId::parse(C).unwrap()
        }
    }
    #[derive(Default)]
    struct Repo {
        values: BTreeMap<ProjectId, Project>,
        fail: bool,
        conflict: bool,
    }
    impl ProjectRepository for Repo {
        fn create(&mut self, p: &Project) -> CoreResult<()> {
            p.validate()?;
            if self.values.contains_key(&p.id) {
                return Err(ProjectCoreError::AlreadyExists(p.id.clone()));
            }
            self.values.insert(p.id.clone(), p.clone());
            Ok(())
        }
        fn get(&self, id: &ProjectId) -> CoreResult<Project> {
            self.values
                .get(id)
                .cloned()
                .ok_or_else(|| ProjectCoreError::NotFound(id.clone()))
        }
        fn list(&self) -> CoreResult<Vec<Project>> {
            let mut v: Vec<_> = self.values.values().cloned().collect();
            v.sort_by(|a, b| {
                b.updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.id.cmp(&b.id))
            });
            Ok(v)
        }
        fn replace(&mut self, p: &Project, e: &Timestamp) -> CoreResult<()> {
            if self.fail {
                return Err(ProjectCoreError::AtomicWriteFailed);
            }
            if self.conflict || self.get(&p.id)?.updated_at != *e {
                return Err(ProjectCoreError::Conflict {
                    project_id: p.id.clone(),
                });
            }
            p.validate()?;
            self.values.insert(p.id.clone(), p.clone());
            Ok(())
        }
        fn delete(&mut self, id: &ProjectId) -> CoreResult<()> {
            self.values
                .remove(id)
                .map(|_| ())
                .ok_or_else(|| ProjectCoreError::NotFound(id.clone()))
        }
    }
    #[derive(Default)]
    struct Store {
        materials: BTreeMap<(ProjectId, MaterialId), Vec<u8>>,
        creations: BTreeMap<(ProjectId, CreationId), Vec<u8>>,
        invalid_path: bool,
        fail_remove: bool,
        deleted: Vec<ProjectId>,
    }
    fn hash(_: &[u8]) -> Sha256Digest {
        Sha256Digest::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap()
    }
    impl ProjectContentStore for Store {
        fn store_material(
            &mut self,
            p: &ProjectId,
            m: &MaterialId,
            s: &MaterialContent,
            file: &str,
        ) -> CoreResult<StoredMaterial> {
            self.materials
                .insert((p.clone(), m.clone()), s.bytes.clone());
            let path = if self.invalid_path {
                "publish/x".into()
            } else {
                format!("inputs/{m}/{file}")
            };
            Ok(StoredMaterial {
                relative_path: RelativeProjectPath::parse(path)?,
                byte_size: s.bytes.len() as u64,
                sha256: hash(&s.bytes),
            })
        }
        fn read_material(&self, p: &ProjectId, m: &Material) -> CoreResult<Vec<u8>> {
            self.materials
                .get(&(p.clone(), m.id.clone()))
                .cloned()
                .ok_or_else(|| ProjectCoreError::NotFound(p.clone()))
        }
        fn store_creation(
            &mut self,
            p: &ProjectId,
            c: &CreationId,
            x: &CreationContent,
            file: &str,
        ) -> CoreResult<StoredCreation> {
            self.creations
                .insert((p.clone(), c.clone()), x.bytes.clone());
            let path = if self.invalid_path {
                "inputs/x".into()
            } else {
                format!("outputs/{c}/{file}")
            };
            Ok(StoredCreation {
                relative_path: RelativeProjectPath::parse(path)?,
                byte_size: x.bytes.len() as u64,
            })
        }
        fn read_creation(&self, p: &ProjectId, c: &Creation) -> CoreResult<Vec<u8>> {
            self.creations
                .get(&(p.clone(), c.id.clone()))
                .cloned()
                .ok_or_else(|| ProjectCoreError::NotFound(p.clone()))
        }
        fn remove_project_tree(&mut self, p: &ProjectId) -> CoreResult<()> {
            self.deleted.push(p.clone());
            if self.fail_remove {
                Err(ProjectCoreError::WriteFailed)
            } else {
                Ok(())
            }
        }
    }
    fn service() -> ProjectService<Repo, Store, TestClock, TestIds> {
        ProjectService::new(Repo::default(), Store::default(), TestClock, TestIds)
    }
    fn material() -> AddMaterial {
        AddMaterial {
            display_name: "Guide".into(),
            original_file_name: "Guía de clase.pdf".into(),
            content_type: Some(ContentType::parse("application/pdf").unwrap()),
            source: MaterialContent {
                bytes: b"original bytes".to_vec(),
            },
        }
    }
    #[test]
    fn values_reject_noncanonical_ids_dates_names_and_paths() {
        assert!(ProjectId::parse(P).is_ok());
        assert!(ProjectId::parse("0198e4a6-6e70-6c01-8c0e-8b6fd26f1f22").is_err());
        assert!(Timestamp::parse("2025-02-29T15:00:00Z").is_err());
        assert!(ProjectName::parse("../secret").is_err());
        for p in ["/a", "inputs/../x", "inputs\\x", "inputs//x"] {
            assert!(RelativeProjectPath::parse(p).is_err())
        }
    }
    #[test]
    fn sanitized_file_name_cannot_be_used_as_a_path() {
        assert_eq!(
            safe_file_name(" ../Guía de clase.pdf "),
            "Gu-a-de-clase.pdf"
        );
        assert_eq!(safe_file_name("..."), "file")
    }
    #[test]
    fn project_lifecycle_is_pure_and_deterministic() {
        let mut s = service();
        let p = s.create_project(" Fotosíntesis ").unwrap();
        assert_eq!(p.name.as_str(), "Fotosíntesis");
        assert_eq!(s.open_project(&pid()).unwrap(), p);
        let renamed = s.rename_project(&pid(), "Sistema solar").unwrap();
        assert_eq!(s.list_projects().unwrap(), vec![renamed]);
        assert!(matches!(
            s.create_project("two"),
            Err(ProjectCoreError::AlreadyExists(_))
        ))
    }
    #[test]
    fn material_is_committed_after_content_and_keeps_fixed_root() {
        let mut s = service();
        s.create_project("one").unwrap();
        let item = s.add_material(&pid(), material()).unwrap();
        assert_eq!(
            item.relative_path.as_str(),
            "inputs/0198e4a6-79b2-7b51-9e68-c2eb7af3db14/Gu-a-de-clase.pdf"
        );
        assert_eq!(
            s.read_material(&pid(), &item.id).unwrap(),
            b"original bytes"
        );
        assert_eq!(s.open_project(&pid()).unwrap().materials, vec![item])
    }
    #[test]
    fn creation_is_outputs_only_revision_one_and_readable() {
        let mut s = service();
        s.create_project("one").unwrap();
        let c = s
            .create_creation(
                &pid(),
                CreateCreation {
                    display_name: "Activity".into(),
                    kind: CreationKind::Web,
                    content_type: Some(ContentType::parse("text/html").unwrap()),
                    content: CreationContent {
                        bytes: b"<h1>x</h1>".to_vec(),
                        file_name: "index.html".into(),
                    },
                    parent_creation_id: None,
                },
            )
            .unwrap();
        assert_eq!(
            c.relative_path.as_str(),
            "outputs/0198e4a6-86d6-7c16-b4c4-3197b355cf10/index.html"
        );
        assert_eq!(c.revision, 1);
        assert_eq!(s.read_creation(&pid(), &c.id).unwrap(), b"<h1>x</h1>")
    }
    #[test]
    fn failed_metadata_replace_leaves_no_content_reference() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (mut r, c, k, i) = s.into_parts();
        r.fail = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.add_material(&pid(), material()),
            Err(ProjectCoreError::AtomicWriteFailed)
        ));
        assert!(s.open_project(&pid()).unwrap().materials.is_empty())
    }
    #[test]
    fn content_store_cannot_substitute_publish_or_inputs_roots() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (r, mut c, k, i) = s.into_parts();
        c.invalid_path = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.add_material(&pid(), material()),
            Err(ProjectCoreError::InvalidPath(_))
        ))
    }
    #[test]
    fn aggregate_rejects_duplicate_ids_and_path_escape() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        let id = MaterialId::parse(M).unwrap();
        let m = Material {
            id: id.clone(),
            display_name: "x".into(),
            original_file_name: "x.txt".into(),
            relative_path: RelativeProjectPath::parse(format!("inputs/{id}/x.txt")).unwrap(),
            content_type: None,
            byte_size: 0,
            sha256: hash(b""),
            created_at: time(),
        };
        p.materials = vec![m.clone(), m];
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::DuplicateMaterial(_))
        ));
        p.materials[1].id = MaterialId::parse("0198e4a6-79b2-7b51-9e68-c2eb7af3db15").unwrap();
        p.materials[1].relative_path = RelativeProjectPath::parse("publish/x/x.txt").unwrap();
        assert!(p.validate().is_err())
    }
    #[test]
    fn delete_checks_existence_before_targeting_tree() {
        let mut s = service();
        assert!(matches!(
            s.delete_project(&pid()),
            Err(ProjectCoreError::NotFound(_))
        ));
        s.create_project("one").unwrap();
        assert!(s.delete_project(&pid()).is_ok());
        assert!(matches!(
            s.open_project(&pid()),
            Err(ProjectCoreError::NotFound(_))
        ))
    }
    #[test]
    fn delete_removes_metadata_before_tree_and_never_restores_it() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (r, mut c, k, i) = s.into_parts();
        c.fail_remove = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.delete_project(&pid()),
            Err(ProjectCoreError::WriteFailed)
        ));
        assert!(matches!(
            s.open_project(&pid()),
            Err(ProjectCoreError::NotFound(_))
        ))
    }
    #[test]
    fn missing_material_and_creation_have_distinct_typed_errors() {
        let mut s = service();
        s.create_project("one").unwrap();
        assert!(matches!(
            s.read_material(&pid(), &MaterialId::parse(M).unwrap()),
            Err(ProjectCoreError::MissingMaterial(_))
        ));
        assert!(matches!(
            s.read_creation(&pid(), &CreationId::parse(C).unwrap()),
            Err(ProjectCoreError::MissingCreation(_))
        ))
    }
    #[test]
    fn create_and_rename_reject_blank_overlong_and_path_like_names() {
        let mut s = service();
        assert!(matches!(
            s.create_project(" "),
            Err(ProjectCoreError::InvalidName(_))
        ));
        assert!(matches!(
            s.create_project("x".repeat(MAX_PROJECT_NAME_CHARS + 1)),
            Err(ProjectCoreError::InvalidName(_))
        ));
        s.create_project("one").unwrap();
        assert!(matches!(
            s.rename_project(&pid(), "a/b"),
            Err(ProjectCoreError::InvalidName(_))
        ))
    }
    #[test]
    fn rename_conflict_preserves_existing_project() {
        let mut s = service();
        s.create_project("one").unwrap();
        let (mut r, c, k, i) = s.into_parts();
        r.conflict = true;
        let mut s = ProjectService::new(r, c, k, i);
        assert!(matches!(
            s.rename_project(&pid(), "two"),
            Err(ProjectCoreError::Conflict { .. })
        ));
        assert_eq!(s.open_project(&pid()).unwrap().name.as_str(), "one")
    }
    #[test]
    fn parent_creation_reference_must_exist_in_aggregate() {
        let mut p = Project::new(pid(), ProjectName::parse("one").unwrap(), time());
        p.creations.push(Creation {
            id: CreationId::parse(C).unwrap(),
            display_name: "x".into(),
            kind: CreationKind::File,
            relative_path: RelativeProjectPath::parse(format!("outputs/{C}/x.txt")).unwrap(),
            content_type: None,
            byte_size: 0,
            revision: 1,
            parent_creation_id: Some(
                CreationId::parse("0198e4a6-86d6-7c16-b4c4-3197b355cf11").unwrap(),
            ),
            created_at: time(),
        });
        assert!(matches!(
            p.validate(),
            Err(ProjectCoreError::InvalidCreation(_))
        ))
    }
}
