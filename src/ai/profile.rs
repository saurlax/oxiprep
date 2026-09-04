//! Local agent launch profiles and platform preferences.

use directories::ProjectDirs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const PROFILE_SCHEMA_VERSION: u32 = 1;
pub const CODEX_PROFILE_ID: &str = "builtin.codex-acp";
pub const CODEX_INSTALL_GUIDANCE: &str =
    "Install manually: npm install -g @agentclientprotocol/codex-acp";
pub const CODEX_NPX_GUIDANCE: &str =
    "Or set command to npx with arguments: -y @agentclientprotocol/codex-acp";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkingDirectoryPolicy {
    #[default]
    SavedProject,
    Application,
    Fixed {
        path: PathBuf,
    },
}

impl WorkingDirectoryPolicy {
    pub fn resolve(
        &self,
        project_path: Option<&Path>,
        application_dir: &Path,
    ) -> Result<PathBuf, ProfileError> {
        let candidate = match self {
            Self::SavedProject => project_path
                .and_then(Path::parent)
                .unwrap_or(application_dir),
            Self::Application => application_dir,
            Self::Fixed { path } => path,
        };
        let absolute = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            application_dir.join(candidate)
        };
        if matches!(self, Self::Fixed { .. }) && !absolute.is_dir() {
            return Err(ProfileError::InvalidWorkingDirectory(absolute));
        }
        absolute
            .canonicalize()
            .map_err(|_| ProfileError::InvalidWorkingDirectory(absolute.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub working_directory: WorkingDirectoryPolicy,
}

impl AgentProfile {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        let mut bytes = [0_u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        Self {
            id: format!("local.{}", hex(&bytes)),
            name: name.into(),
            command: command.into(),
            args: Vec::new(),
            environment: BTreeMap::new(),
            working_directory: WorkingDirectoryPolicy::default(),
        }
    }

    pub fn codex() -> Self {
        Self {
            id: CODEX_PROFILE_ID.to_owned(),
            name: "Codex".to_owned(),
            command: "codex-acp".to_owned(),
            args: Vec::new(),
            environment: BTreeMap::new(),
            working_directory: WorkingDirectoryPolicy::SavedProject,
        }
    }

    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.id.trim().is_empty() || contains_nul(&self.id) {
            return Err(ProfileError::InvalidId);
        }
        if self.name.trim().is_empty() || contains_nul(&self.name) {
            return Err(ProfileError::InvalidName);
        }
        if self.command.trim().is_empty() || contains_nul(&self.command) {
            return Err(ProfileError::InvalidCommand);
        }
        if self.args.iter().any(|value| contains_nul(value)) {
            return Err(ProfileError::InvalidArgument);
        }
        if self.environment.iter().any(|(name, value)| {
            name.is_empty() || name.contains('=') || contains_nul(name) || contains_nul(value)
        }) {
            return Err(ProfileError::InvalidEnvironment);
        }
        Ok(())
    }

    pub fn resolved(
        &self,
        project_path: Option<&Path>,
        application_dir: &Path,
    ) -> Result<ResolvedProfile, ProfileError> {
        self.validate()?;
        Ok(ResolvedProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            environment: self.environment.clone(),
            working_directory: self
                .working_directory
                .resolve(project_path, application_dir)?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedProfile {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub working_directory: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileError {
    InvalidId,
    InvalidName,
    InvalidCommand,
    InvalidArgument,
    InvalidEnvironment,
    InvalidWorkingDirectory(PathBuf),
    Io(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId => f.write_str("Profile ID is empty or contains a NUL character."),
            Self::InvalidName => f.write_str("Profile name is empty or contains a NUL character."),
            Self::InvalidCommand => {
                f.write_str("Executable command is empty or contains a NUL character.")
            }
            Self::InvalidArgument => f.write_str("An argument contains a NUL character."),
            Self::InvalidEnvironment => f.write_str("An environment override is invalid."),
            Self::InvalidWorkingDirectory(path) => {
                write!(f, "Working directory is not available: {}", path.display())
            }
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ProfileError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfilePreferences {
    pub version: u32,
    #[serde(default)]
    profiles: Vec<AgentProfile>,
    #[serde(default)]
    deleted_builtins: BTreeSet<String>,
}

impl Default for ProfilePreferences {
    fn default() -> Self {
        Self {
            version: PROFILE_SCHEMA_VERSION,
            profiles: Vec::new(),
            deleted_builtins: BTreeSet::new(),
        }
    }
}

impl ProfilePreferences {
    pub fn effective_profiles(&self) -> Vec<AgentProfile> {
        let mut profiles = Vec::new();
        if !self.deleted_builtins.contains(CODEX_PROFILE_ID) {
            profiles.push(
                self.profiles
                    .iter()
                    .find(|profile| profile.id == CODEX_PROFILE_ID)
                    .cloned()
                    .unwrap_or_else(AgentProfile::codex),
            );
        }
        profiles.extend(
            self.profiles
                .iter()
                .filter(|profile| profile.id != CODEX_PROFILE_ID)
                .cloned(),
        );
        profiles
    }

    pub fn upsert(&mut self, profile: AgentProfile) -> Result<(), ProfileError> {
        profile.validate()?;
        self.deleted_builtins.remove(&profile.id);
        if let Some(existing) = self
            .profiles
            .iter_mut()
            .find(|existing| existing.id == profile.id)
        {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        Ok(())
    }

    pub fn delete(&mut self, id: &str) {
        self.profiles.retain(|profile| profile.id != id);
        if id == CODEX_PROFILE_ID {
            self.deleted_builtins.insert(id.to_owned());
        }
    }

    pub fn reset_codex(&mut self) {
        self.profiles
            .retain(|profile| profile.id != CODEX_PROFILE_ID);
        self.deleted_builtins.remove(CODEX_PROFILE_ID);
    }
}

pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    pub fn platform() -> Result<Self, ProfileError> {
        let dirs = ProjectDirs::from("com", "saurlax", "Oxiprep").ok_or_else(|| {
            ProfileError::Io("The platform configuration directory is unavailable.".to_owned())
        })?;
        Ok(Self::at(dirs.config_dir().join("agent-profiles.json")))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> (ProfilePreferences, Option<String>) {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (ProfilePreferences::default(), None);
            }
            Err(error) => {
                return (
                    ProfilePreferences::default(),
                    Some(format!("Could not read agent profiles: {error}")),
                );
            }
        };
        match serde_json::from_slice::<ProfilePreferences>(&bytes) {
            Ok(preferences) if preferences.version == PROFILE_SCHEMA_VERSION => (preferences, None),
            Ok(_) => (
                ProfilePreferences::default(),
                Some("Agent profile preferences use an unsupported version.".to_owned()),
            ),
            Err(error) => (
                ProfilePreferences::default(),
                Some(format!("Agent profile preferences are invalid: {error}")),
            ),
        }
    }

    pub fn save(&self, preferences: &ProfilePreferences) -> Result<(), ProfileError> {
        if preferences.version != PROFILE_SCHEMA_VERSION {
            return Err(ProfileError::Io(
                "Cannot save an unsupported profile version.".to_owned(),
            ));
        }
        for profile in &preferences.profiles {
            profile.validate()?;
        }
        let parent = self.path.parent().ok_or_else(|| {
            ProfileError::Io("Agent profile path has no parent directory.".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(io_error)?;

        let mut random = [0_u8; 8];
        rand::rng().fill_bytes(&mut random);
        let temp = parent.join(format!(".agent-profiles-{}.tmp", hex(&random)));
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)
                .map_err(io_error)?;
            serde_json::to_writer_pretty(&mut file, preferences)
                .map_err(|error| ProfileError::Io(error.to_string()))?;
            file.write_all(b"\n").map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
            fs::rename(&temp, &self.path).map_err(io_error)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["TOKEN", "KEY", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL"]
        .iter()
        .any(|needle| upper.contains(needle))
}

pub fn display_environment(environment: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    environment
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                if is_secret_name(name) {
                    "••••••••".to_owned()
                } else {
                    value.clone()
                },
            )
        })
        .collect()
}

pub fn redacted_launch_error(profile: &AgentProfile, error: &io::Error) -> String {
    format!(
        "Could not launch {} ({}): {error}",
        profile.name, profile.command
    )
}

fn contains_nul(value: &str) -> bool {
    value.as_bytes().contains(&0)
}

fn io_error(error: io::Error) -> ProfileError {
    ProfileError::Io(error.to_string())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_preserves_arguments_and_rejects_empty_or_nul_commands() {
        let mut profile = AgentProfile::new("Test", "agent");
        profile.args = vec![
            "two words".to_owned(),
            "*.step".to_owned(),
            "$HOME".to_owned(),
        ];
        assert!(profile.validate().is_ok());
        assert_eq!(profile.args, ["two words", "*.step", "$HOME"]);
        profile.command.clear();
        assert_eq!(profile.validate(), Err(ProfileError::InvalidCommand));
        profile.command = "bad\0command".to_owned();
        assert_eq!(profile.validate(), Err(ProfileError::InvalidCommand));
    }

    #[test]
    fn resolves_every_working_directory_policy_and_unsaved_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let project_dir = temp.path().join("project");
        fs::create_dir(&project_dir).unwrap();
        let project = project_dir.join("part.oxiprep");
        let application = temp.path().canonicalize().unwrap();

        assert_eq!(
            WorkingDirectoryPolicy::SavedProject
                .resolve(Some(&project), &application)
                .unwrap(),
            project_dir.canonicalize().unwrap()
        );
        assert_eq!(
            WorkingDirectoryPolicy::SavedProject
                .resolve(None, &application)
                .unwrap(),
            application
        );
        assert_eq!(
            WorkingDirectoryPolicy::Application
                .resolve(Some(&project), &application)
                .unwrap(),
            application
        );
        assert_eq!(
            (WorkingDirectoryPolicy::Fixed {
                path: project_dir.clone()
            })
            .resolve(None, &application)
            .unwrap(),
            project_dir.canonicalize().unwrap()
        );
    }

    #[test]
    fn rejects_invalid_fixed_path_and_resolves_relative_path_absolutely() {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().canonicalize().unwrap();
        assert!(matches!(
            (WorkingDirectoryPolicy::Fixed {
                path: PathBuf::from("missing")
            })
            .resolve(None, &app),
            Err(ProfileError::InvalidWorkingDirectory(_))
        ));
        fs::create_dir(app.join("valid")).unwrap();
        let resolved = (WorkingDirectoryPolicy::Fixed {
            path: PathBuf::from("valid"),
        })
        .resolve(None, &app)
        .unwrap();
        assert!(resolved.is_absolute());
    }

    #[test]
    fn preferences_round_trip_and_corrupt_file_recovers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config").join("profiles.json");
        let store = ProfileStore::at(path.clone());
        assert_eq!(store.load(), (ProfilePreferences::default(), None));
        let mut preferences = ProfilePreferences::default();
        preferences
            .upsert(AgentProfile::new("Other", "other-agent"))
            .unwrap();
        store.save(&preferences).unwrap();
        assert_eq!(store.load(), (preferences, None));
        fs::write(&path, b"not json").unwrap();
        let (recovered, warning) = store.load();
        assert_eq!(recovered, ProfilePreferences::default());
        assert!(warning.unwrap().contains("invalid"));
    }

    #[test]
    fn codex_profile_can_be_overridden_deleted_and_reset() {
        let mut preferences = ProfilePreferences::default();
        assert_eq!(preferences.effective_profiles(), [AgentProfile::codex()]);
        let mut override_profile = AgentProfile::codex();
        override_profile.command = "npx".to_owned();
        override_profile.args = vec!["-y".to_owned(), "@agentclientprotocol/codex-acp".to_owned()];
        preferences.upsert(override_profile.clone()).unwrap();
        assert_eq!(preferences.effective_profiles(), [override_profile]);
        preferences.delete(CODEX_PROFILE_ID);
        assert!(preferences.effective_profiles().is_empty());
        preferences.reset_codex();
        assert_eq!(preferences.effective_profiles(), [AgentProfile::codex()]);
        assert!(CODEX_INSTALL_GUIDANCE.contains("npm install -g"));
        assert!(CODEX_NPX_GUIDANCE.contains("npx"));
    }

    #[test]
    fn secret_values_are_masked_and_absent_from_launch_diagnostics() {
        let mut environment = BTreeMap::new();
        for name in ["API_TOKEN", "api_key", "Password", "client_secret"] {
            environment.insert(name.to_owned(), format!("value-for-{name}"));
        }
        environment.insert("RUST_LOG".to_owned(), "debug".to_owned());
        let display = display_environment(&environment);
        assert!(
            display
                .iter()
                .all(|(name, value)| name == "RUST_LOG" && value == "debug" || value == "••••••••")
        );

        let mut profile = AgentProfile::codex();
        profile.environment = environment;
        let message = redacted_launch_error(
            &profile,
            &io::Error::new(io::ErrorKind::NotFound, "not found"),
        );
        assert!(!message.contains("value-for"));
        assert!(message.contains("not found"));
    }
}
