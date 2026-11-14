//! Bounded, request-local expansion of Cloud Identity group invitations.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use openidconnect::AccessToken;
use openidconnect::reqwest;
use schisaciaccole_core::api::{Attendee, Meeting, ResponseStatus};
use schisaciaccole_core::attendee::{
    GroupExpansion, GroupExpansionOutcome, GroupMember, merge_group_attendees,
};
use serde::Deserialize;
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use crate::app::AppState;
use crate::config::GOOGLE_GROUPS_SCOPE;
use crate::error::ApiFailure;
use crate::meetings::access_token;
use crate::store;

const TOTAL_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUESTS: usize = 64;
const MAX_DEPTH: usize = 8;
const MAX_PAGES: usize = 10;
const MAX_RECORDS: usize = 5_000;
const MAX_BODY_BYTES: usize = 2_000_000;

/// An intentionally opaque failure: response bodies, URLs and member identities
/// never enter errors or logs. Every failure keeps the original invitation.
#[derive(Clone, Copy, Debug)]
struct Incomplete;

#[derive(Deserialize)]
struct Lookup {
    name: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Member {
    preferred_member_key: MemberKey,
    #[serde(rename = "type", default)]
    kind: String,
}

#[derive(Clone, Deserialize)]
struct MemberKey {
    id: String,
    #[serde(default)]
    namespace: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MembershipPage {
    #[serde(default)]
    memberships: Vec<Member>,
    next_page_token: Option<String>,
}

/// A resolver belongs to exactly one authenticated HTTP request. Completed
/// network results are reused across its meetings, including failed lookups.
struct Resolver<'a> {
    http: &'a reqwest::Client,
    base_url: &'a str,
    token: AccessToken,
    deadline: Instant,
    requests: usize,
    records: usize,
    staged_leaves: usize,
    lookups: HashMap<String, Result<Option<String>, Incomplete>>,
    memberships: HashMap<String, Result<Vec<Member>, Incomplete>>,
}

impl<'a> Resolver<'a> {
    fn new(http: &'a reqwest::Client, base_url: &'a str, token: AccessToken) -> Self {
        Self {
            http,
            base_url,
            token,
            deadline: Instant::now() + TOTAL_TIMEOUT,
            requests: 0,
            records: 0,
            staged_leaves: 0,
            lookups: HashMap::new(),
            memberships: HashMap::new(),
        }
    }

    /// Expands a whole root privately. A cycle or any incomplete descendant
    /// discards all leaves; empty groups successfully produce no people.
    ///
    /// Returns `Ok(None)` when Cloud Identity answers 404 for the root itself:
    /// the invitation is not a group and stays a direct attendee. A 404 for a
    /// nested member of unknown type is still incomplete.
    async fn expand(&mut self, email: &str) -> Result<Option<Vec<GroupMember>>, Incomplete> {
        let mut leaves = Vec::new();
        let mut pending = vec![(email.to_string(), 0, HashSet::new())];
        let mut visited = HashSet::new();
        while let Some((email, depth, mut ancestors)) = pending.pop() {
            if depth >= MAX_DEPTH || Instant::now() >= self.deadline {
                return Err(Incomplete);
            }
            let group = match self.lookup(&email).await? {
                Some(group) => group,
                None if depth == 0 => return Ok(None),
                None => return Err(Incomplete),
            };
            if !ancestors.insert(group.clone()) {
                return Err(Incomplete);
            }
            if !visited.insert(group.clone()) {
                continue;
            }
            let members = self.members(&group).await?;
            for member in members {
                if !member.preferred_member_key.namespace.is_empty()
                    || !valid_email(&member.preferred_member_key.id)
                {
                    return Err(Incomplete);
                }
                if member.kind == "USER" {
                    if self.staged_leaves >= MAX_RECORDS {
                        return Err(Incomplete);
                    }
                    self.staged_leaves += 1;
                    leaves.push(GroupMember {
                        email: member.preferred_member_key.id,
                        name: None,
                    });
                } else {
                    pending.push((member.preferred_member_key.id, depth + 1, ancestors.clone()));
                }
            }
        }
        Ok(Some(leaves))
    }

    /// Looks up the group resource name of an address. `Ok(None)` means Cloud
    /// Identity answered 404, so the address is not a visible group.
    async fn lookup(&mut self, email: &str) -> Result<Option<String>, Incomplete> {
        let key = email.trim().to_ascii_lowercase();
        if let Some(result) = self.lookups.get(&key) {
            return result.clone();
        }
        let result = self.lookup_uncached(email).await;
        self.lookups.insert(key, result.clone());
        result
    }

    async fn lookup_uncached(&mut self, email: &str) -> Result<Option<String>, Incomplete> {
        if !valid_email(email) {
            return Err(Incomplete);
        }
        let url = endpoint(self.base_url, &["groups:lookup"])?;
        let Some(lookup) = self.fetch::<Lookup>(url, &[("groupKey.id", email)]).await? else {
            return Ok(None);
        };
        if !valid_group_name(&lookup.name) {
            return Err(Incomplete);
        }
        Ok(Some(lookup.name))
    }

    async fn members(&mut self, group: &str) -> Result<Vec<Member>, Incomplete> {
        if let Some(result) = self.memberships.get(group) {
            return result.clone();
        }
        let result = self.members_uncached(group).await;
        self.memberships.insert(group.to_string(), result.clone());
        result
    }

    async fn members_uncached(&mut self, group: &str) -> Result<Vec<Member>, Incomplete> {
        let Some(id) = group.strip_prefix("groups/") else {
            return Err(Incomplete);
        };
        let url = endpoint(self.base_url, &["groups", id, "memberships"])?;
        let mut members = Vec::new();
        let mut token: Option<String> = None;
        let mut tokens = HashSet::new();
        for _ in 0..MAX_PAGES {
            let mut query = vec![("view", "FULL"), ("pageSize", "500")];
            if let Some(token) = token.as_deref() {
                query.push(("pageToken", token));
            }
            let Some(page) = self.fetch::<MembershipPage>(url.clone(), &query).await? else {
                return Err(Incomplete);
            };
            self.records += page.memberships.len();
            if self.records > MAX_RECORDS {
                return Err(Incomplete);
            }
            members.extend(page.memberships);
            match page.next_page_token.filter(|token| !token.is_empty()) {
                None => return Ok(members),
                Some(next) => {
                    if !tokens.insert(next.clone()) {
                        return Err(Incomplete);
                    }
                    token = Some(next);
                }
            }
        }
        Err(Incomplete)
    }

    /// Bounds sending and body reads together. A 404 gives `Ok(None)` without
    /// reading the body; every other non-success status and invalid JSON are
    /// indistinguishable to callers and never expose provider content.
    async fn fetch<T: serde::de::DeserializeOwned>(
        &mut self,
        url: reqwest::Url,
        query: &[(&str, &str)],
    ) -> Result<Option<T>, Incomplete> {
        if self.requests >= MAX_REQUESTS || Instant::now() >= self.deadline {
            return Err(Incomplete);
        }
        self.requests += 1;
        let request_deadline = self.deadline.min(Instant::now() + REQUEST_TIMEOUT);
        timeout_at(request_deadline, async {
            let mut response = self
                .http
                .get(url)
                .bearer_auth(self.token.secret())
                .query(query)
                .send()
                .await
                .map_err(|_| Incomplete)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if !response.status().is_success() {
                return Err(Incomplete);
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| Incomplete)? {
                if bytes.len() + chunk.len() > MAX_BODY_BYTES {
                    return Err(Incomplete);
                }
                bytes.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| Incomplete)
        })
        .await
        .map_err(|_| Incomplete)?
    }
}

/// Resolves every invitation in `meetings` with one shared five-second budget.
/// Missing group consent leaves Calendar meetings usable and explicitly marks
/// reauthorization; permission/API failures merely mark resolution incomplete.
/// Only attendees that could be groups are looked up (see `needs_lookup`), and
/// an address Cloud Identity does not know (404) stays a direct attendee, so a
/// meeting of people only is never marked incomplete.
///
/// # Errors
///
/// Returns database failures when reading identity or grants. Token failures are
/// treated as incomplete expansion so successful Calendar data remains usable.
pub(crate) async fn resolve_meetings(
    state: &AppState,
    user_id: Uuid,
    meetings: &mut [Meeting],
) -> Result<(), ApiFailure> {
    let deadline = Instant::now() + TOTAL_TIMEOUT;
    if !meetings.iter().any(has_lookup_candidates) {
        return Ok(());
    }
    let loaded = timeout_at(deadline, async {
        let user = store::find_user(&state.pool, user_id)
            .await?
            .ok_or(ApiFailure::Unauthorized)?;
        let credentials = store::google_credentials(&state.pool, user_id).await?;
        Ok::<_, ApiFailure>((user, credentials))
    })
    .await;
    let (user, credentials) = match loaded {
        Ok(result) => result?,
        Err(_) => {
            for meeting in meetings
                .iter_mut()
                .filter(|meeting| has_lookup_candidates(meeting))
            {
                mark_incomplete(meeting, "", false);
            }
            return Ok(());
        }
    };
    let granted = credentials.is_some_and(|credentials| {
        credentials
            .scopes
            .split_whitespace()
            .any(|scope| scope == GOOGLE_GROUPS_SCOPE)
    });
    if !granted {
        for meeting in meetings
            .iter_mut()
            .filter(|meeting| has_lookup_candidates(meeting))
        {
            mark_incomplete(meeting, &user.email, true);
        }
        return Ok(());
    }
    let token = timeout_at(deadline, access_token(state, user_id)).await;
    let Ok(Ok(token)) = token else {
        for meeting in meetings
            .iter_mut()
            .filter(|meeting| has_lookup_candidates(meeting))
        {
            mark_incomplete(meeting, &user.email, false);
        }
        return Ok(());
    };
    let credentials = timeout_at(deadline, store::google_credentials(&state.pool, user_id)).await;
    let reauthorization_required = match credentials {
        Ok(Ok(credentials)) => !credentials.is_some_and(|credentials| {
            credentials
                .scopes
                .split_whitespace()
                .any(|scope| scope == GOOGLE_GROUPS_SCOPE)
        }),
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => {
            for meeting in meetings
                .iter_mut()
                .filter(|meeting| has_lookup_candidates(meeting))
            {
                mark_incomplete(meeting, &user.email, false);
            }
            return Ok(());
        }
    };
    if reauthorization_required {
        for meeting in meetings
            .iter_mut()
            .filter(|meeting| has_lookup_candidates(meeting))
        {
            mark_incomplete(meeting, &user.email, true);
        }
        return Ok(());
    }
    let mut resolver = Resolver::new(
        state.google.http(),
        &state.config.google_groups_base_url,
        token,
    );
    resolver.deadline = deadline;
    for meeting in meetings {
        let mut expansions = Vec::with_capacity(meeting.attendees.len());
        for attendee in &meeting.attendees {
            if !needs_lookup(attendee) {
                continue;
            }
            let result = timeout_at(resolver.deadline, resolver.expand(&attendee.email)).await;
            let outcome = match result {
                Ok(Ok(Some(members))) => GroupExpansionOutcome::Complete(members),
                Ok(Ok(None)) => continue,
                _ => GroupExpansionOutcome::Incomplete {
                    reauthorization_required: false,
                },
            };
            expansions.push(GroupExpansion {
                email: attendee.email.clone(),
                outcome,
            });
        }
        let resolved = merge_group_attendees(&meeting.attendees, &expansions, &user.email);
        meeting.attendees = resolved.attendees;
        meeting.participant_resolution = resolved.resolution;
    }
    Ok(())
}

/// True when `attendee` could be a group invitation. A group cannot answer an
/// invitation, so the signed-in user and anyone who accepted, declined or
/// answered maybe is a person and needs no Cloud Identity lookup.
fn needs_lookup(attendee: &Attendee) -> bool {
    !attendee.is_self && attendee.response == ResponseStatus::NeedsAction
}

/// True when at least one attendee of `meeting` needs a group lookup.
fn has_lookup_candidates(meeting: &Meeting) -> bool {
    meeting.attendees.iter().any(needs_lookup)
}

fn mark_incomplete(meeting: &mut Meeting, email: &str, reauthorization_required: bool) {
    let resolved = merge_group_attendees(&meeting.attendees, &[], email);
    meeting.attendees = resolved.attendees;
    meeting.participant_resolution.incomplete = true;
    meeting.participant_resolution.reauthorization_required = reauthorization_required;
}

fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !email
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
}

fn valid_group_name(name: &str) -> bool {
    name.strip_prefix("groups/").is_some_and(|id| {
        !id.is_empty()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    })
}

fn endpoint(base_url: &str, segments: &[&str]) -> Result<reqwest::Url, Incomplete> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| Incomplete)?;
    url.path_segments_mut()
        .map_err(|_| Incomplete)?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn lookup(server: &MockServer, email: &str, id: &str) {
        Mock::given(method("GET"))
            .and(path("/groups:lookup"))
            .and(query_param("groupKey.id", email))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"name": format!("groups/{id}")})),
            )
            .expect(1)
            .mount(server)
            .await;
    }

    async fn members(server: &MockServer, id: &str, body: Value) {
        Mock::given(method("GET"))
            .and(path(format!("/groups/{id}/memberships")))
            .and(query_param("view", "FULL"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(server)
            .await;
    }

    fn user(email: &str) -> Value {
        json!({"preferredMemberKey": {"id": email}, "type": "USER"})
    }

    fn group(email: &str) -> Value {
        json!({"preferredMemberKey": {"id": email}, "type": "GROUP"})
    }

    #[tokio::test]
    async fn follows_pages_and_trusts_explicit_user_members() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        Mock::given(method("GET"))
            .and(path("/groups/team/memberships"))
            .and(query_param("view", "FULL"))
            .and(query_param_is_missing("pageToken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"memberships":[user("ada@example.com")], "nextPageToken":"two"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/groups/team/memberships"))
            .and(query_param("pageToken", "two"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"memberships":[user("bob@example.com")]})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        let leaves = resolver.expand("team@example.com").await.unwrap().unwrap();
        assert_eq!(leaves.len(), 2);
        assert_eq!(resolver.requests, 3);
    }

    #[tokio::test]
    async fn nested_and_shared_groups_reuse_only_request_local_network_results() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        lookup(&server, "shared@example.com", "shared").await;
        members(
            &server,
            "team",
            json!({"memberships":[group("shared@example.com")]}),
        )
        .await;
        members(
            &server,
            "shared",
            json!({"memberships":[user("ada@example.com")]}),
        )
        .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert_eq!(
            resolver
                .expand("team@example.com")
                .await
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            resolver
                .expand("shared@example.com")
                .await
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(resolver.requests, 4);
    }

    #[tokio::test]
    async fn cycles_discard_all_staged_people() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        members(
            &server,
            "team",
            json!({"memberships":[user("ada@example.com"),group("team@example.com")]}),
        )
        .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team@example.com").await.is_err());
    }

    #[tokio::test]
    async fn unknown_members_require_lookup_and_404_is_incomplete() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        members(
            &server,
            "team",
            json!({"memberships":[{"preferredMemberKey":{"id":"unknown@example.com"}}]}),
        )
        .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team@example.com").await.is_err());
        assert_eq!(resolver.requests, 3);
    }

    #[tokio::test]
    async fn empty_group_is_a_complete_empty_expansion() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        members(&server, "team", json!({})).await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(
            resolver
                .expand("team@example.com")
                .await
                .unwrap()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn repeated_page_token_rolls_back_previous_page() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        Mock::given(method("GET"))
            .and(path("/groups/team/memberships"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"memberships":[user("ada@example.com")],"nextPageToken":"repeat"}),
            ))
            .expect(2)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team@example.com").await.is_err());
        assert_eq!(resolver.requests, 3);
    }

    #[tokio::test]
    async fn provider_failures_are_opaque_and_memoized() {
        for status in [401, 403, 429, 500, 503] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/groups:lookup"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_string("private email and token"),
                )
                .expect(1)
                .mount(&server)
                .await;
            let http = reqwest::Client::new();
            let base_url = server.uri();
            let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
            assert!(resolver.expand("team@example.com").await.is_err());
            assert!(resolver.expand("team@example.com").await.is_err());
            assert_eq!(resolver.requests, 1);
        }
    }

    #[tokio::test]
    async fn invalid_json_resource_paths_and_members_fail_conservatively() {
        for body in [
            json!({"name":"groups/../secret"}),
            json!({"name":"groups/id/other"}),
            json!({"name":42}),
            json!({}),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/groups:lookup"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .expect(1)
                .mount(&server)
                .await;
            let http = reqwest::Client::new();
            let base_url = server.uri();
            let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
            assert!(resolver.expand("team@example.com").await.is_err());
            assert_eq!(resolver.requests, 1);
        }
        for member in [
            json!({"preferredMemberKey":{"id":"not-email"},"type":"USER"}),
            json!({"preferredMemberKey":{"id":"ada@example.com","namespace":"external"},"type":"USER"}),
            json!({"type":"USER"}),
        ] {
            let server = MockServer::start().await;
            lookup(&server, "team@example.com", "team").await;
            members(&server, "team", json!({"memberships":[member]})).await;
            let http = reqwest::Client::new();
            let base_url = server.uri();
            let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
            assert!(resolver.expand("team@example.com").await.is_err());
        }
    }

    #[tokio::test]
    async fn request_and_record_budgets_block_further_network_work() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        resolver.requests = MAX_REQUESTS;
        assert!(resolver.expand("team@example.com").await.is_err());
        assert_eq!(resolver.requests, MAX_REQUESTS);
        assert!(server.received_requests().await.unwrap().is_empty());

        resolver.requests = 0;
        resolver.records = MAX_RECORDS;
        lookup(&server, "records@example.com", "records").await;
        members(
            &server,
            "records",
            json!({"memberships":[user("ada@example.com")]}),
        )
        .await;
        assert!(resolver.expand("records@example.com").await.is_err());
    }

    #[tokio::test]
    async fn nesting_limit_does_not_publish_any_leaf() {
        let server = MockServer::start().await;
        for index in 0..MAX_DEPTH {
            lookup(
                &server,
                &format!("team{index}@example.com"),
                &format!("team{index}"),
            )
            .await;
            members(&server, &format!("team{index}"), json!({"memberships":[user("ada@example.com"),group(&format!("team{}@example.com",index+1))]})).await;
        }
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team0@example.com").await.is_err());
        assert_eq!(resolver.requests, MAX_DEPTH * 2);
    }

    #[tokio::test]
    async fn page_limit_rejects_an_unfinished_group() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        for index in 0..MAX_PAGES {
            let mock = Mock::given(method("GET")).and(path("/groups/team/memberships"));
            let mock = if index == 0 {
                mock.and(query_param_is_missing("pageToken"))
            } else {
                mock.and(query_param("pageToken", index.to_string()))
            };
            mock.respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"nextPageToken":(index+1).to_string()})),
            )
            .expect(1)
            .mount(&server)
            .await;
        }
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team@example.com").await.is_err());
        assert_eq!(resolver.requests, MAX_PAGES + 1);
    }

    #[tokio::test]
    async fn authenticated_requests_do_not_share_lookup_or_membership_results() {
        use wiremock::matchers::header;
        let server = MockServer::start().await;
        for (token, email) in [("user-a", "ada@example.com"), ("user-b", "bob@example.com")] {
            Mock::given(method("GET"))
                .and(path("/groups:lookup"))
                .and(header("authorization", format!("Bearer {token}")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"name":"groups/team"})),
                )
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("GET"))
                .and(path("/groups/team/memberships"))
                .and(header("authorization", format!("Bearer {token}")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({"memberships":[user(email)]})),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        let http = reqwest::Client::new();
        let base_url = server.uri();
        for (token, email) in [("user-a", "ada@example.com"), ("user-b", "bob@example.com")] {
            let mut resolver = Resolver::new(&http, &base_url, AccessToken::new(token.into()));
            let leaves = resolver.expand("team@example.com").await.unwrap().unwrap();
            assert_eq!(leaves.len(), 1);
            assert_eq!(leaves[0].email, email);
        }
    }

    #[tokio::test]
    async fn oversized_and_unparseable_responses_fail_without_provider_details() {
        for body in ["{".to_string(), "x".repeat(MAX_BODY_BYTES + 1)] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/groups:lookup"))
                .respond_with(ResponseTemplate::new(200).set_body_string(body))
                .expect(1)
                .mount(&server)
                .await;
            let http = reqwest::Client::new();
            let base_url = server.uri();
            let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
            let error = resolver.expand("team@example.com").await.unwrap_err();
            assert_eq!(format!("{error:?}"), "Incomplete");
        }
    }

    #[tokio::test]
    async fn an_oversized_membership_page_cannot_publish_partial_members() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        let people: Vec<_> = (0..=MAX_RECORDS)
            .map(|index| user(&format!("person{index}@example.com")))
            .collect();
        members(&server, "team", json!({"memberships": people})).await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team@example.com").await.is_err());
        assert_eq!(resolver.staged_leaves, 0);
    }

    #[tokio::test]
    async fn request_timeout_applies_before_the_shared_deadline() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/groups:lookup"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(REQUEST_TIMEOUT + Duration::from_secs(1))
                    .set_body_json(json!({"name":"groups/team"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        let started = Instant::now();
        assert!(resolver.expand("team@example.com").await.is_err());
        assert!(started.elapsed() < TOTAL_TIMEOUT);
        assert!(Instant::now() < resolver.deadline);
    }

    #[tokio::test]
    async fn cached_memberships_still_consume_the_shared_staged_leaf_budget() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        members(
            &server,
            "team",
            json!({"memberships":[user("ada@example.com")]}),
        )
        .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert_eq!(
            resolver
                .expand("team@example.com")
                .await
                .unwrap()
                .unwrap()
                .len(),
            1
        );
        resolver.staged_leaves = MAX_RECORDS;
        assert!(resolver.expand("team@example.com").await.is_err());
        assert_eq!(resolver.requests, 2);
        assert_eq!(resolver.records, 1);
    }

    #[tokio::test]
    async fn shared_deadline_and_per_request_timeout_bound_slow_responses() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/groups:lookup"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(200))
                    .set_body_json(json!({"name":"groups/team"})),
            )
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        resolver.deadline = Instant::now() + Duration::from_millis(25);
        let started = Instant::now();
        assert!(resolver.expand("team@example.com").await.is_err());
        assert!(started.elapsed() < Duration::from_millis(180));
        assert!(resolver.expand("other@example.com").await.is_err());
        assert_eq!(resolver.requests, 1);
    }

    #[tokio::test]
    async fn root_unknown_to_cloud_identity_is_not_a_group_and_is_memoized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/groups:lookup"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("ada@example.com").await.unwrap().is_none());
        assert!(resolver.expand("ada@example.com").await.unwrap().is_none());
        assert_eq!(resolver.requests, 1);
    }

    #[tokio::test]
    async fn nested_group_member_unknown_to_cloud_identity_is_incomplete() {
        let server = MockServer::start().await;
        lookup(&server, "team@example.com", "team").await;
        members(
            &server,
            "team",
            json!({"memberships":[group("gone@example.com")]}),
        )
        .await;
        Mock::given(method("GET"))
            .and(path("/groups:lookup"))
            .and(query_param("groupKey.id", "gone@example.com"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let base_url = server.uri();
        let mut resolver = Resolver::new(&http, &base_url, AccessToken::new("secret".into()));
        assert!(resolver.expand("team@example.com").await.is_err());
    }

    fn attendee(is_self: bool, response: ResponseStatus) -> Attendee {
        Attendee {
            email: "ada@example.com".to_string(),
            name: String::new(),
            is_self,
            optional: false,
            response,
        }
    }

    #[test]
    fn attendee_without_an_answer_needs_lookup() {
        assert!(needs_lookup(&attendee(false, ResponseStatus::NeedsAction)));
    }

    #[test]
    fn attendee_who_answered_is_a_person() {
        for response in [
            ResponseStatus::Accepted,
            ResponseStatus::Declined,
            ResponseStatus::Tentative,
        ] {
            assert!(!needs_lookup(&attendee(false, response)));
        }
    }

    #[test]
    fn signed_in_user_is_never_looked_up() {
        assert!(!needs_lookup(&attendee(true, ResponseStatus::NeedsAction)));
    }
}
