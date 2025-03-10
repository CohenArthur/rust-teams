use crate::data::Data;
use crate::github::GitHubApi;
use crate::schema::{Email, Permissions, Team, TeamKind, ZulipGroupMember};
use crate::zulip::ZulipApi;
use failure::{bail, Error};
use log::{error, warn};
use regex::Regex;
use std::collections::{HashMap, HashSet};

macro_rules! checks {
    ($($f:ident,)*) => {
        &[$(
            Check {
                f: $f,
                name: stringify!($f)
            }
        ),*]
    }
}

#[allow(clippy::type_complexity)]
static CHECKS: &[Check<fn(&Data, &mut Vec<String>)>] = checks![
    validate_name_prefixes,
    validate_subteam_of,
    validate_team_leads,
    validate_team_members,
    validate_alumni,
    validate_inactive_members,
    validate_list_email_addresses,
    validate_list_extra_people,
    validate_list_extra_teams,
    validate_list_addresses,
    validate_people_addresses,
    validate_duplicate_permissions,
    validate_permissions,
    validate_rfcbot_labels,
    validate_rfcbot_exclude_members,
    validate_team_names,
    validate_github_teams,
    validate_zulip_stream_name,
    validate_project_groups_have_parent_teams,
    validate_discord_team_members_have_discord_ids,
    validate_zulip_group_ids,
    validate_zulip_group_extra_people,
    validate_repos,
];

#[allow(clippy::type_complexity)]
static GITHUB_CHECKS: &[Check<fn(&Data, &GitHubApi, &mut Vec<String>)>] =
    checks![validate_github_usernames,];

#[allow(clippy::type_complexity)]
static ZULIP_CHECKS: &[Check<fn(&Data, &ZulipApi, &mut Vec<String>)>] =
    checks![validate_zulip_users,];

struct Check<F> {
    f: F,
    name: &'static str,
}

pub(crate) fn validate(data: &Data, strict: bool, skip: &[&str]) -> Result<(), Error> {
    let mut errors = Vec::new();

    for check in CHECKS {
        if skip.contains(&check.name) {
            warn!("skipped check: {}", check.name);
            continue;
        }

        (check.f)(data, &mut errors);
    }

    let github = GitHubApi::new();
    if let Err(err) = github.require_auth() {
        if strict {
            return Err(err);
        } else {
            warn!("couldn't perform checks relying on the GitHub API, some errors will not be detected");
            warn!("cause: {}", err);
        }
    } else {
        for check in GITHUB_CHECKS {
            if skip.contains(&check.name) {
                warn!("skipped check: {}", check.name);
                continue;
            }

            (check.f)(data, &github, &mut errors);
        }
    }

    let zulip = ZulipApi::new();
    if let Err(err) = zulip.require_auth() {
        warn!("couldn't perform checks relying on the Zulip API, some errors will not be detected");
        warn!("cause: {}", err);
    } else {
        for check in ZULIP_CHECKS {
            if skip.contains(&check.name) {
                warn!("skipped check: {}", check.name);
                continue;
            }

            (check.f)(data, &zulip, &mut errors);
        }
    }

    if !errors.is_empty() {
        errors.sort();
        errors.dedup_by(|a, b| a == b);

        for err in &errors {
            error!("validation error: {}", err);
        }

        bail!("{} validation errors found", errors.len());
    }

    Ok(())
}

/// Ensure working group names start with `wg-`
fn validate_name_prefixes(data: &Data, errors: &mut Vec<String>) {
    fn ensure_prefix(
        team: &Team,
        kind: TeamKind,
        prefix: &str,
        exceptions: &[&str],
    ) -> Result<(), Error> {
        if exceptions.contains(&team.name()) {
            return Ok(());
        }
        if team.kind() == kind && !team.name().starts_with(prefix) {
            bail!(
                "{} `{}`'s name doesn't start with `{}`",
                kind,
                team.name(),
                prefix,
            );
        } else if team.kind() != kind && team.name().starts_with(prefix) {
            bail!(
                "{} `{}` seems like a {} (since it has the `{}` prefix)",
                team.kind(),
                team.name(),
                kind,
                prefix,
            );
        }
        Ok(())
    }
    wrapper(data.teams(), errors, |team, _| {
        ensure_prefix(team, TeamKind::WorkingGroup, "wg-", &["wg-leads"])?;
        ensure_prefix(
            team,
            TeamKind::ProjectGroup,
            "project-",
            &["project-group-leads"],
        )?;
        Ok(())
    });
}

/// Ensure `subteam-of` points to an existing team
fn validate_subteam_of(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |mut team, _| {
        let mut visited = Vec::new();
        while let Some(parent) = team.subteam_of() {
            visited.push(team.name());

            if visited.contains(&parent) {
                bail!(
                    "team `{parent}` is a subteam of itself: {} => {parent}",
                    visited.join(" => "),
                );
            }

            let Some(parent) = data.team(parent) else {
                bail!(
                    "the parent of team `{}` doesn't exist: `{}`",
                    team.name(),
                    parent,
                );
            };

            if !matches!(team.kind(), TeamKind::Team) && parent.subteam_of().is_some() {
                bail!(
                    "{} `{}` can't be a subteam of a subteam (`{}`)",
                    team.kind(),
                    team.name(),
                    parent.name(),
                );
            }

            team = parent;
        }
        Ok(())
    });
}

/// Ensure team leaders are part of the teams they lead
fn validate_team_leads(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        let members = team.members(data)?;
        wrapper(team.leads().iter(), errors, |lead, _| {
            if !members.contains(lead) {
                bail!(
                    "`{}` leads team `{}`, but is not a member of it",
                    lead,
                    team.name()
                );
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure team members are people
fn validate_team_members(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(team.members(data)?.iter(), errors, |member, _| {
            if data.person(member).is_none() {
                bail!(
                    "person `{}` is member of team `{}` but doesn't exist",
                    member,
                    team.name()
                );
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure alumni are not active
fn validate_alumni(data: &Data, errors: &mut Vec<String>) {
    let active_members = match data.active_members() {
        Ok(ms) => ms,
        Err(e) => {
            errors.push(e.to_string());
            return;
        }
    };
    wrapper(data.team("alumni").iter(), errors, |alumni_team, errors| {
        let mut explicit_members: HashSet<_> = alumni_team.explicit_members().iter().collect();
        // Ensure alumni team members are not active
        wrapper(alumni_team.members(data)?.iter(), errors, |member, _| {
            if active_members.contains(member) {
                bail!("alumni team includes active member '{}'", member)
            }
            Ok(())
        });
        // Ensure all explicitly named members of the alumni team are not also implicitly members
        wrapper(
            data.teams()
                .filter(|t| t.name() != "alumni")
                .flat_map(|t| t.alumni().iter().map(move |m| (t.name(), m))),
            errors,
            |(team, member), _| {
                if explicit_members.remove(member) {
                    bail!("alumni team explicitly includes member '{}' who was specified as an alumni already in '{}'", member, team)
                }
                Ok(())
            },
        );
        Ok(())
    });
}

/// Ensure every person is part of at least one team (active or archived)
fn validate_inactive_members(data: &Data, errors: &mut Vec<String>) {
    let mut referenced_members = HashSet::new();
    wrapper(
        data.teams().chain(data.archived_teams()),
        errors,
        |team, _| {
            let members = team.members(data)?;
            for member in members {
                referenced_members.insert(member);
            }
            for person in team.alumni() {
                referenced_members.insert(person);
            }
            for list in team.raw_lists() {
                for person in &list.extra_people {
                    referenced_members.insert(person);
                }
            }
            Ok(())
        },
    );

    let all_members = data.people().map(|p| p.github()).collect::<HashSet<_>>();
    // All the individual contributors to any Rust controlled repos
    let all_ics = data
        .repos()
        .flat_map(|r| r.access.individuals.keys())
        .map(|n| n.as_str())
        .collect::<HashSet<_>>();
    wrapper(
        all_members.difference(&referenced_members),
        errors,
        |person, _| {
            if !data.person(person).unwrap().permissions().has_any() && !all_ics.contains(person) {
                bail!(
                    "person `{}` is not a member of any team (active or archived), has no permissions, and is not an individual contributor to any repo",
                    person
                );
            }
            Ok(())
        },
    );
}

/// Ensure every member of a team with a mailing list has an email address
fn validate_list_email_addresses(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        if team.lists(data)?.is_empty() {
            return Ok(());
        }
        wrapper(team.members(data)?.iter(), errors, |member, _| {
            if let Some(member) = data.person(member) {
                if let Email::Missing = member.email() {
                    bail!(
                        "person `{}` is a member of a mailing list but has no email address",
                        member.github()
                    );
                }
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure members of extra-people in a list are real people
fn validate_list_extra_people(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(team.raw_lists().iter(), errors, |list, _| {
            for person in &list.extra_people {
                if data.person(person).is_none() {
                    bail!(
                        "person `{}` does not exist (in list `{}`)",
                        person,
                        list.address
                    );
                }
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure members of extra-people in a list are real people
fn validate_list_extra_teams(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(team.raw_lists().iter(), errors, |list, _| {
            for list_team in &list.extra_teams {
                if data.team(list_team).is_none() {
                    bail!(
                        "team `{}` does not exist (in list `{}`)",
                        list_team,
                        list.address
                    );
                }
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure the list addresses are correct
fn validate_list_addresses(data: &Data, errors: &mut Vec<String>) {
    let email_re = Regex::new(r"^[a-zA-Z0-9_\.-]+@([a-zA-Z0-9_\.-]+)$").unwrap();
    let config = data.config().allowed_mailing_lists_domains();
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(team.raw_lists().iter(), errors, |list, _| {
            if let Some(captures) = email_re.captures(&list.address) {
                if !config.contains(&captures[1]) {
                    bail!("list address on a domain we don't own: `{}`", list.address);
                }
            } else {
                bail!("invalid list address: `{}`", list.address);
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure people email addresses are correct
fn validate_people_addresses(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.people(), errors, |person, _| {
        if let Email::Present(email) = person.email() {
            if !email.contains('@') {
                bail!("invalid email address of `{}`: {}", person.github(), email);
            }
        }
        Ok(())
    });
}

/// Ensure members of teams with permissions don't explicitly have those permissions
fn validate_duplicate_permissions(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(team.members(data)?.iter(), errors, |member, _| {
            if let Some(person) = data.person(member) {
                for permission in &Permissions::available(data.config()) {
                    if team.permissions().has(permission)
                        && person.permissions().has_directly(permission)
                    {
                        bail!(
                            "user `{}` has the permission `{}` both explicitly and through \
                             the `{}` team",
                            member,
                            permission,
                            team.name()
                        );
                    }
                }
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure the permissions are valid
fn validate_permissions(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, _| {
        team.permissions()
            .validate(format!("team `{}`", team.name()), data.config())?;
        team.leads_permissions()
            .validate(format!("team `{}`", team.name()), data.config())?;
        Ok(())
    });
    wrapper(data.people(), errors, |person, _| {
        person
            .permissions()
            .validate(format!("user `{}`", person.github()), data.config())?;
        Ok(())
    });
}

/// Ensure there are no duplicate rfcbot labels
fn validate_rfcbot_labels(data: &Data, errors: &mut Vec<String>) {
    let mut labels = HashSet::new();
    wrapper(data.teams(), errors, move |team, errors| {
        if let Some(rfcbot) = team.rfcbot_data() {
            if !labels.insert(rfcbot.label.clone()) {
                errors.push(format!("duplicate rfcbot label: {}", rfcbot.label));
            }
        }
        Ok(())
    });
}

/// Ensure rfcbot's exclude-members only contains not duplicated team members
fn validate_rfcbot_exclude_members(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, move |team, errors| {
        if let Some(rfcbot) = team.rfcbot_data() {
            let mut exclude = HashSet::new();
            let members = team.members(data)?;
            wrapper(rfcbot.exclude_members.iter(), errors, move |member, _| {
                if !exclude.insert(member) {
                    bail!(
                        "duplicate member in `{}` rfcbot.exclude-members: {}",
                        team.name(),
                        member
                    );
                }
                if !members.contains(member.as_str()) {
                    bail!(
                        "person `{}` is not a member of team `{}` (in rfcbot.exclude-members)",
                        member,
                        team.name()
                    );
                }
                Ok(())
            });
        }
        Ok(())
    });
}

/// Ensure team names are alphanumeric + `-`
fn validate_team_names(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, _| {
        if !team.name().chars().all(|c| c.is_alphanumeric() || c == '-') {
            bail!(
                "team name `{}` can only be alphanumeric with dashes",
                team.name()
            );
        }
        Ok(())
    });
}

/// Ensure GitHub teams are unique and in the allowed orgs
fn validate_github_teams(data: &Data, errors: &mut Vec<String>) {
    let mut found = HashMap::new();
    let allowed = data.config().allowed_github_orgs();
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(
            team.github_teams(data)?.into_iter(),
            errors,
            |gh_team, _| {
                if !allowed.contains(gh_team.org) {
                    bail!(
                        "GitHub organization `{}` isn't allowed (in team `{}`)",
                        gh_team.org,
                        team.name()
                    );
                }
                if let Some(other) = found.insert((gh_team.org, gh_team.name), team.name()) {
                    bail!(
                        "GitHub team `{}/{}` is defined for both the `{}` and `{}` teams",
                        gh_team.org,
                        gh_team.name,
                        team.name(),
                        other
                    );
                }
                Ok(())
            },
        );
        Ok(())
    });
}

/// Ensure there are no misspelled GitHub account names
fn validate_github_usernames(data: &Data, github: &GitHubApi, errors: &mut Vec<String>) {
    let people = data
        .people()
        .map(|p| (p.github_id(), p))
        .collect::<HashMap<_, _>>();
    match github.usernames(&people.keys().cloned().collect::<Vec<_>>()) {
        Ok(res) => wrapper(res.iter(), errors, |(id, name), _| {
            let original = people[id].github();
            if original != name {
                bail!("user `{}` changed username to `{}`", original, name);
            }
            Ok(())
        }),
        Err(err) => errors.push(format!("couldn't verify GitHub usernames: {}", err)),
    }
}

/// Ensure the user doens't put an URL as the Zulip stream name.
fn validate_zulip_stream_name(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, _| {
        if let Some(stream) = team.website_data().and_then(|ws| ws.zulip_stream()) {
            if stream.starts_with("https://") {
                bail!(
                    "the zulip stream name of the team `{}` is a link: only the name is required",
                    team.name()
                );
            }
        }
        Ok(())
    })
}

/// Ensure each project group has a parent team, according to RFC 2856.
fn validate_project_groups_have_parent_teams(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, _| {
        if team.kind() == TeamKind::ProjectGroup && team.subteam_of().is_none() {
            bail!(
                "the project group `{}` doesn't have a parent team, but it's required to have one",
                team.name()
            );
        }
        Ok(())
    })
}

fn validate_discord_team_members_have_discord_ids(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, _| {
        if team.discord_roles().is_some() && team.name() != "all" {
            let team_members = team.members(data)?;
            if team_members.len() != team.discord_ids(data)?.len() {
                let members: String = team_members
                    .into_iter()
                    .filter(|name| data.person(name).map(|p| p.discord_id()) == Some(None))
                    .map(|name| format!("{}, ", name))
                    .collect();

                bail!(
                    "the following members of the \"{}\" team do not have discord_ids: {}",
                    team.name(),
                    &members[..members.len() - 2]
                );
            }
        }

        Ok(())
    });
}

/// Ensure every member of a team that has a Zulip group has a Zulip id
fn validate_zulip_users(data: &Data, zulip: &ZulipApi, errors: &mut Vec<String>) {
    let by_id = match zulip.get_users() {
        Ok(u) => u.iter().map(|u| u.user_id).collect::<HashSet<_>>(),
        Err(err) => {
            errors.push(format!("couldn't verify Zulip users: {}", err));
            return;
        }
    };
    let zulip_groups = match data.zulip_groups() {
        Ok(zgs) => zgs,
        Err(err) => {
            errors.push(format!("couldn't get all the Zulip groups: {}", err));
            return;
        }
    };
    wrapper(zulip_groups.iter(), errors, |(group_name, group), _| {
        let missing_members = group
            .members()
            .iter()
            .filter_map(|m| match m {
                ZulipGroupMember::MemberWithId { github, zulip_id }
                    if !by_id.contains(zulip_id) =>
                {
                    Some(github.clone())
                }
                ZulipGroupMember::JustId(zulip_id) if !by_id.contains(zulip_id) => {
                    Some(format!("ID: {zulip_id}"))
                }
                ZulipGroupMember::MemberWithoutId { github } => Some(github.clone()),
                _ => None,
            })
            .collect::<HashSet<_>>();
        if !missing_members.is_empty() {
            bail!(
                "the \"{}\" Zulip group includes members who don't appear on Zulip: {}",
                group_name,
                missing_members.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
        Ok(())
    })
}

/// Ensure every member of a team that has a Zulip group either has a Zulip id
fn validate_zulip_group_ids(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        let groups = team.zulip_groups(data)?;
        // Returns if group is empty or all the groups don't include the team members
        if groups.is_empty() || groups.iter().all(|g| !g.includes_team_members()) {
            return Ok(());
        }
        wrapper(team.members(data)?.iter(), errors, |member, _| {
            if let Some(member) = data.person(member) {
                if member.zulip_id().is_none() {
                    bail!(
                        "person `{}` in '{}' is a member of a Zulip user group but has no Zulip id",
                        member.github(),
                        team.name()
                    );
                }
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure members of extra-people in a Zulip user group are real people
fn validate_zulip_group_extra_people(data: &Data, errors: &mut Vec<String>) {
    wrapper(data.teams(), errors, |team, errors| {
        wrapper(team.raw_zulip_groups().iter(), errors, |group, _| {
            for person in &group.extra_people {
                if data.person(person).is_none() {
                    bail!(
                        "person `{}` does not exist (in Zulip group `{}`)",
                        person,
                        group.name
                    );
                }
            }
            Ok(())
        });
        Ok(())
    });
}

/// Ensure repos reference valid teams
fn validate_repos(data: &Data, errors: &mut Vec<String>) {
    let allowed_orgs = data.config().allowed_github_orgs();
    let github_teams = data.github_teams();
    wrapper(data.repos(), errors, |repo, _| {
        if !allowed_orgs.contains(&repo.org) {
            bail!(
                "The repo '{}' is in an invalid org '{}'",
                repo.name,
                repo.org
            );
        }
        for (team_name, _) in &repo.access.teams {
            if !github_teams.contains(&(repo.org.clone(), team_name.clone())) {
                bail!(
                        "access for {}/{} is invalid: '{}' is not configured as a GitHub team for the '{}' org",
                        repo.org,
                        repo.name,
                        team_name,
                        repo.org
                    )
            }
        }

        for (name, _) in &repo.access.individuals {
            if data.person(name).is_none() {
                bail!(
                    "access for {}/{} is invalid: '{}' is not the name of a person in the team repo",
                    repo.org,
                    repo.name,
                    name
                );
            }
        }
        Ok(())
    });
}

fn wrapper<T, I, F>(iter: I, errors: &mut Vec<String>, mut func: F)
where
    I: Iterator<Item = T>,
    F: FnMut(T, &mut Vec<String>) -> Result<(), Error>,
{
    for item in iter {
        if let Err(err) = func(item, errors) {
            errors.push(err.to_string());
        }
    }
}
