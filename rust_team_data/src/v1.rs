use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub static BASE_URL: &str = "https://team-api.infra.rust-lang.org/v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamKind {
    Team,
    WorkingGroup,
    ProjectGroup,
    MarkerTeam,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub name: String,
    pub kind: TeamKind,
    pub subteam_of: Option<String>,
    pub members: Vec<TeamMember>,
    pub alumni: Vec<TeamMember>,
    pub github: Option<TeamGitHub>,
    pub website_data: Option<TeamWebsite>,
    pub discord: Vec<TeamDiscord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub name: String,
    pub github: String,
    pub github_id: usize,
    pub is_lead: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamGitHub {
    pub teams: Vec<GitHubTeam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubTeam {
    pub org: String,
    pub name: String,
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamWebsite {
    pub name: String,
    pub description: String,
    pub page: String,
    pub email: Option<String>,
    pub repo: Option<String>,
    pub discord: Option<DiscordInvite>,
    pub zulip_stream: Option<String>,
    pub weight: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamDiscord {
    pub name: String,
    pub members: Vec<usize>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInvite {
    pub channel: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Teams {
    #[serde(flatten)]
    pub teams: IndexMap<String, Team>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repos {
    #[serde(flatten)]
    pub repos: IndexMap<String, Vec<Repo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct List {
    pub address: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lists {
    pub lists: IndexMap<String, List>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipGroup {
    pub name: String,
    pub members: Vec<ZulipGroupMember>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZulipGroupMember {
    // TODO(rylev): this variant can be removed once
    // it is verified that noone is relying on it
    Email(String),
    Id(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipGroups {
    pub groups: IndexMap<String, ZulipGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub github_users: Vec<String>,
    pub github_ids: Vec<usize>,
    pub discord_ids: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rfcbot {
    pub teams: IndexMap<String, RfcbotTeam>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfcbotTeam {
    pub name: String,
    pub ping: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZulipMapping {
    /// Zulip ID to GitHub ID
    pub users: IndexMap<usize, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub org: String,
    pub name: String,
    pub description: String,
    pub bots: Vec<Bot>,
    pub teams: Vec<RepoTeam>,
    pub members: Vec<RepoMember>,
    pub branch_protections: Vec<BranchProtection>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Bot {
    Bors,
    Highfive,
    Rustbot,
    RustTimer,
    Rfcbot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoTeam {
    pub name: String,
    pub permission: RepoPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoMember {
    pub name: String,
    pub permission: RepoPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoPermission {
    Write,
    Admin,
    Maintain,
    Triage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchProtection {
    pub pattern: String,
    pub ci_checks: Vec<String>,
    pub dismiss_stale_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub name: String,
    pub email: Option<String>,
    pub github_id: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct People {
    /// GitHub name as key.
    pub people: IndexMap<String, Person>,
}
