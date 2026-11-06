//! In-process [`SiloApi`] that behaves like a small Silo server: token rotation, expiry, revocation, PIN profiles,
//! profile-restricted libraries. Used by unit tests, the desktop host's tests and UI development without a server
//! (ADR-0004 "Consequences"). Data is synthetic.
use crate::api::{Auth, ImageData, SiloApi};
use crate::catalog::*;
use crate::error::{ApiError, Result};
use crate::models::*;
use crate::secret::Secret;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

/// Problem type URI for a problem id, as the server builds it (`apiv2/problem.go@6791aac` L22).
pub fn problem(status: u16, id: &str) -> ApiError {
    ApiError::Http {
        status,
        problem: Some(Box::new(Problem {
            type_uri: format!("https://siloserver.org/docs/api/v2/problems/{id}"),
            title: id.into(),
            status: i64::from(status),
            detail: String::new(),
            instance: "urn:silo:request:mock".into(),
        })),
        retry_after: None,
    }
}

#[derive(Default)]
struct State {
    counter: u64,
    access_lifetime: i64,
    live_access: HashSet<String>,
    /// refresh token → user id
    live_refresh: HashMap<String, String>,
    /// profile token → profile id
    profile_tokens: HashMap<String, String>,
    refresh_calls: u32,
    unauthorized: u32,
    refresh_503: bool,
}

pub struct MockSiloApi {
    pub info: SystemInfo,
    pub server_id: String,
    users: Vec<(String, String, Account)>,
    profiles: Vec<Profile>,
    pins: HashMap<String, String>,
    libraries: Vec<UserLibrary>,
    /// profile id → allowed library ids (absent: all)
    restrictions: HashMap<String, Vec<String>>,
    state: Mutex<State>,
}

fn profile(id: &str, name: &str, has_pin: bool, is_child: bool) -> Profile {
    Profile {
        id: id.into(),
        name: name.into(),
        has_pin,
        is_child,
        is_primary: id == "1",
        avatar_url: None,
        language: "en".into(),
        subtitle_language: String::new(),
        show_forced_subtitles: true,
        auto_skip_intro: false,
        auto_skip_credits: false,
        auto_skip_recap: false,
        max_playback_quality: String::new(),
    }
}

fn library(id: &str, name: &str, kind: &str, order: i64) -> UserLibrary {
    UserLibrary {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        sort_order: order,
        box_sets_enabled: false,
        poster_url: None,
    }
}

impl MockSiloApi {
    /// A server with user `alice`/`pw`, profiles Alice (no PIN) and Kids (PIN 1234, one library), two libraries.
    pub fn demo() -> Self {
        let account = Account {
            id: "1".into(),
            username: "alice".into(),
            email: String::new(),
            role: "user".into(),
            permissions: Vec::new(),
            download_allowed: true,
            password_change_required: false,
        };
        Self {
            info: SystemInfo {
                api_major: 2,
                server_version: "mock".into(),
                contract_digest: crate::TESTED_CONTRACT_DIGESTS[0].into(),
                links: SystemInfoLinks {
                    openapi: "/api/v2/openapi.json".into(),
                    capabilities: "/api/v2/capabilities".into(),
                    identity: "/api/v2/system/identity".into(),
                },
                request_origin: None,
            },
            server_id: "3f2a9d5e-6b1c-4c7e-9a0d-2f4b8c1e7a35".into(),
            users: vec![("alice".into(), "pw".into(), account)],
            profiles: vec![profile("1", "Alice", false, false), profile("2", "Kids", true, true)],
            pins: HashMap::from([("2".to_owned(), "1234".to_owned())]),
            libraries: vec![library("1", "Movies", "movies", 0), library("2", "Shows", "series", 1)],
            restrictions: HashMap::from([("2".to_owned(), vec!["2".to_owned()])]),
            state: Mutex::new(State {
                access_lifetime: 3600,
                ..State::default()
            }),
        }
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        match self.state.lock() {
            Ok(mut s) => f(&mut s),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    pub fn set_access_lifetime(&self, secs: i64) {
        self.with_state(|s| s.access_lifetime = secs);
    }
    /// Every issued access token becomes invalid (as after expiry).
    pub fn expire_access_tokens(&self) {
        self.with_state(|s| s.live_access.clear());
    }
    /// Every login session is revoked: refresh answers 401.
    pub fn revoke_all_sessions(&self) {
        self.with_state(|s| {
            s.live_access.clear();
            s.live_refresh.clear();
        });
    }
    pub fn fail_refresh_with_503(&self, on: bool) {
        self.with_state(|s| s.refresh_503 = on);
    }
    pub fn refresh_calls(&self) -> u32 {
        self.with_state(|s| s.refresh_calls)
    }
    pub fn unauthorized_responses(&self) -> u32 {
        self.with_state(|s| s.unauthorized)
    }
    pub fn live_sessions(&self) -> usize {
        self.with_state(|s| s.live_refresh.len())
    }
    pub fn is_live_refresh(&self, token: &str) -> bool {
        self.with_state(|s| s.live_refresh.contains_key(token))
    }

    fn issue(&self, user_id: &str) -> (Secret, Secret, i64) {
        self.with_state(|s| {
            s.counter += 1;
            let access = format!("mock-access-{}", s.counter);
            let refresh = format!("mock-refresh-{}", s.counter);
            s.live_access.insert(access.clone());
            s.live_refresh.insert(refresh.clone(), user_id.to_owned());
            (Secret::new(access), Secret::new(refresh), s.access_lifetime)
        })
    }

    fn check(&self, auth: &Auth) -> Result<()> {
        self.with_state(|s| {
            if s.live_access.contains(auth.access.expose()) {
                Ok(())
            } else {
                s.unauthorized += 1;
                Err(problem(401, "invalid_token"))
            }
        })
    }

    /// Profile gate: a PIN profile needs a valid profile token.
    /// Invalidate every issued profile token (PIN changed / token expired).
    pub fn revoke_profile_tokens(&self) {
        self.with_state(|s| s.profile_tokens.clear());
    }

    fn check_profile(&self, auth: &Auth) -> Result<Option<String>> {
        let Some(p) = &auth.profile else { return Ok(None) };
        if !self.profiles.iter().any(|x| x.id == p.id) {
            return Err(problem(404, "not_found"));
        }
        if self.pins.contains_key(&p.id) {
            let ok = p
                .token
                .as_ref()
                .is_some_and(|t| self.with_state(|s| s.profile_tokens.get(t.expose()) == Some(&p.id)));
            if !ok {
                return Err(problem(403, "profile_verification_required"));
            }
        }
        Ok(Some(p.id.clone()))
    }
}

/// Synthetic catalog: three movies in library 1, one series (one season, three episodes) in library 2.
mod demo {
    use super::*;

    pub fn art(kind: &str, id: &str) -> Option<String> {
        Some(format!("/api/v2/artwork/{kind}-{id}?exp=4102444800&sig=mock"))
    }

    fn rollup(position: Option<f64>, duration: f64, played: bool) -> WatchRollup {
        WatchRollup {
            played,
            position_seconds: position,
            duration_seconds: Some(duration),
            is_in_progress: Some(position.is_some()),
            ..WatchRollup::default()
        }
    }

    pub struct Movie {
        pub id: &'static str,
        pub title: &'static str,
        pub year: i64,
        pub minutes: i64,
        pub overview: &'static str,
        pub genres: &'static [&'static str],
        pub position: Option<f64>,
        pub played: bool,
    }

    pub const MOVIES: &[Movie] = &[
        Movie {
            id: "m1",
            title: "Northern Lights",
            year: 2023,
            minutes: 118,
            overview: "A night-shift radio host follows a signal across the tundra.",
            genres: &["Drama", "Mystery"],
            position: Some(1520.0),
            played: false,
        },
        Movie {
            id: "m2",
            title: "Harbor Nights",
            year: 2021,
            minutes: 104,
            overview: "Two dock workers stumble on a smuggling ring.",
            genres: &["Thriller"],
            position: None,
            played: true,
        },
        Movie {
            id: "m3",
            title: "Glass Garden",
            year: 2024,
            minutes: 97,
            overview: "A botanist restores a ruined greenhouse and the town around it.",
            genres: &["Comedy", "Drama"],
            position: None,
            played: false,
        },
    ];

    pub fn movie_card(m: &Movie) -> CatalogItem {
        CatalogItem {
            content_id: m.id.into(),
            title: m.title.into(),
            kind: "movie".into(),
            year: Some(m.year),
            overview: Some(m.overview.into()),
            runtime: Some(m.minutes),
            duration_seconds: Some((m.minutes * 60) as f64),
            genres: m.genres.iter().map(|g| (*g).to_owned()).collect(),
            poster_url: art("poster", m.id),
            backdrop_url: art("backdrop", m.id),
            position_seconds: m.position,
            user_state: Some(CatalogItemUserState {
                played: m.played,
                ..CatalogItemUserState::default()
            }),
            ..CatalogItem::default()
        }
    }

    pub fn series_card() -> CatalogItem {
        CatalogItem {
            content_id: "s1".into(),
            title: "The Long Field".into(),
            kind: "series".into(),
            year: Some(2022),
            overview: Some("A farming family weathers one strange year.".into()),
            genres: vec!["Drama".into()],
            poster_url: art("poster", "s1"),
            backdrop_url: art("backdrop", "s1"),
            play_content_id: Some("e2".into()),
            ..CatalogItem::default()
        }
    }

    /// (library id, card) for every top-level item.
    pub fn cards() -> Vec<(&'static str, CatalogItem)> {
        let mut v: Vec<_> = MOVIES.iter().map(|m| ("1", movie_card(m))).collect();
        v.push(("2", series_card()));
        v
    }

    pub fn episodes() -> Vec<Episode> {
        (1..=3)
            .map(|n| Episode {
                content_id: format!("e{n}"),
                title: ["Thaw", "Seed", "Flood"][usize::try_from(n - 1).unwrap_or_default()].into(),
                season_number: 1,
                episode_number: n,
                runtime: 48,
                overview: Some(format!("Episode {n} of the first season.")),
                still_url: art("still", &format!("e{n}")),
                user_data: Some(rollup(if n == 2 { Some(600.0) } else { None }, 2880.0, n == 1)),
                ..Episode::default()
            })
            .collect()
    }

    fn audio(lang: &str, codec: &str, channels: i64, default: bool) -> AudioTrack {
        AudioTrack {
            codec: Some(codec.into()),
            channels: Some(channels),
            language: Some(lang.into()),
            default,
            ..AudioTrack::default()
        }
    }

    fn subtitle(index: i64, lang: &str, forced: bool, default: bool) -> SubtitleTrack {
        SubtitleTrack {
            index: Some(index),
            codec: Some("subrip".into()),
            language: Some(lang.into()),
            forced,
            default,
            ..SubtitleTrack::default()
        }
    }

    pub fn versions(id: &str, seconds: i64) -> Vec<FileVersion> {
        let base = FileVersion {
            file_id: format!("{id}-f1"),
            container: "mkv".into(),
            codec_video: "hevc".into(),
            codec_audio: "eac3".into(),
            resolution: "2160p".into(),
            hdr: true,
            duration_seconds: seconds,
            audio_tracks: vec![audio("eng", "eac3", 6, true), audio("deu", "ac3", 6, false)],
            subtitle_tracks: vec![subtitle(2, "eng", false, false), subtitle(3, "deu", true, false)],
            ..FileVersion::default()
        };
        if id == "m1" {
            let hd = FileVersion {
                file_id: format!("{id}-f2"),
                codec_video: "h264".into(),
                resolution: "1080p".into(),
                hdr: false,
                ..base.clone()
            };
            vec![base, hd]
        } else {
            vec![base]
        }
    }

    pub fn detail(id: &str) -> Option<CatalogItemDetail> {
        if let Some(m) = MOVIES.iter().find(|m| m.id == id) {
            let c = movie_card(m);
            return Some(CatalogItemDetail {
                content_id: c.content_id,
                title: c.title,
                kind: c.kind,
                overview: c.overview,
                year: c.year,
                runtime: c.runtime,
                genres: c.genres,
                cast: vec![
                    CastCredit {
                        name: "Ada Brook".into(),
                        character: "Lead".into(),
                        ..CastCredit::default()
                    },
                    CastCredit {
                        name: "Tom Reyes".into(),
                        character: "Friend".into(),
                        order: 1,
                        ..CastCredit::default()
                    },
                ],
                poster_url: c.poster_url,
                backdrop_url: c.backdrop_url,
                user_data: Some(rollup(m.position, (m.minutes * 60) as f64, m.played)),
                versions: versions(id, m.minutes * 60),
                ..CatalogItemDetail::default()
            });
        }
        if id == "s1" {
            let c = series_card();
            return Some(CatalogItemDetail {
                content_id: c.content_id,
                title: c.title,
                kind: c.kind,
                overview: c.overview,
                year: c.year,
                genres: c.genres,
                poster_url: c.poster_url,
                backdrop_url: c.backdrop_url,
                season_count: Some(1),
                episode_count: Some(3),
                play_content_id: Some("e2".into()),
                play_season_number: Some(1),
                ..CatalogItemDetail::default()
            });
        }
        let e = episodes().into_iter().find(|e| e.content_id == id)?;
        Some(CatalogItemDetail {
            content_id: e.content_id,
            title: e.title,
            kind: "episode".into(),
            overview: e.overview,
            runtime: Some(e.runtime),
            series_id: Some("s1".into()),
            series_title: Some("The Long Field".into()),
            season_number: Some(1),
            episode_number: Some(e.episode_number),
            backdrop_url: e.still_url,
            user_data: e.user_data,
            versions: versions(id, e.runtime * 60),
            ..CatalogItemDetail::default()
        })
    }

    /// Placeholder poster: a 2×3 PNG (scaled up by the UI) whose colour depends on the key, with a lighter top row.
    pub fn png(key: &str) -> Vec<u8> {
        let h = key
            .bytes()
            .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b)));
        let [r, g, b, _] = h.to_le_bytes().map(|c| 40 + c / 3);
        let lighter = [r.saturating_add(30), g.saturating_add(30), b.saturating_add(30)];
        let base = [r, g, b];
        let mut raw = Vec::new();
        for row in 0..3 {
            raw.push(0); // filter: none
            for _ in 0..2 {
                raw.extend_from_slice(if row == 0 { &lighter } else { &base });
            }
        }
        super::png::encode_rgb(2, 3, &raw)
    }
}

impl MockSiloApi {
    fn visible_cards(&self, auth: &Auth) -> Result<Vec<(&'static str, CatalogItem)>> {
        let profile = self
            .check_profile(auth)?
            .ok_or_else(|| problem(400, "profile_required"))?;
        let allowed = self.restrictions.get(&profile);
        Ok(demo::cards()
            .into_iter()
            .filter(|(lib, _)| allowed.is_none_or(|ids| ids.iter().any(|i| i == lib)))
            .collect())
    }
}

#[async_trait]
impl SiloApi for MockSiloApi {
    async fn system_info(&self, _origin: &str) -> Result<SystemInfo> {
        Ok(self.info.clone())
    }

    async fn server_identity(&self, _origin: &str) -> Result<ServerIdentity> {
        Ok(ServerIdentity {
            server_id: self.server_id.clone(),
        })
    }

    async fn login(&self, _origin: &str, username: &str, password: &Secret) -> Result<TokenPair> {
        let Some((_, _, user)) = self
            .users
            .iter()
            .find(|(u, p, _)| u == username && p == password.expose())
        else {
            return Err(ApiError::InvalidCredentials);
        };
        let (access_token, refresh_token, expires_in) = self.issue(&user.id);
        Ok(TokenPair {
            access_token,
            refresh_token,
            expires_in,
            user: user.clone(),
        })
    }

    async fn refresh(&self, _origin: &str, refresh_token: &Secret) -> Result<RefreshedTokens> {
        // Widen the race window so single-flight tests are meaningful.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let user = self.with_state(|s| {
            s.refresh_calls += 1;
            if s.refresh_503 {
                return Err(match problem(503, "dependency_unavailable") {
                    ApiError::Http { status, problem, .. } => ApiError::Http {
                        status,
                        problem,
                        retry_after: Some(5),
                    },
                    other => other,
                });
            }
            s.live_refresh
                .remove(refresh_token.expose())
                .ok_or_else(|| problem(401, "session_expired"))
        })?;
        let (access_token, refresh_token, expires_in) = self.issue(&user);
        Ok(RefreshedTokens {
            access_token,
            refresh_token,
            expires_in,
        })
    }

    async fn logout(&self, _origin: &str, auth: &Auth) -> Result<()> {
        self.check(auth)?;
        self.with_state(|s| {
            s.live_access.remove(auth.access.expose());
            s.live_refresh.clear();
        });
        Ok(())
    }

    async fn current_user(&self, _origin: &str, auth: &Auth) -> Result<Account> {
        self.check(auth)?;
        self.check_profile(auth)?;
        self.users
            .first()
            .map(|(_, _, a)| a.clone())
            .ok_or_else(|| problem(404, "not_found"))
    }

    async fn list_profiles(&self, _origin: &str, auth: &Auth) -> Result<ProfileCollection> {
        self.check(auth)?;
        // Upstream validates a supplied profile token on every authenticated route.
        self.check_profile(auth)?;
        Ok(ProfileCollection {
            items: self.profiles.clone(),
        })
    }

    async fn verify_profile_pin(
        &self,
        _origin: &str,
        auth: &Auth,
        profile_id: &str,
        pin: &Secret,
    ) -> Result<ProfileVerification> {
        self.check(auth)?;
        self.check_profile(auth)?;
        let Some(expected) = self.pins.get(profile_id) else {
            return Err(problem(404, "not_found"));
        };
        if expected != pin.expose() {
            return Ok(ProfileVerification {
                valid: false,
                profile_token: None,
                expires_at: None,
            });
        }
        let token = self.with_state(|s| {
            s.counter += 1;
            let t = format!("pvt_mock{}", s.counter);
            s.profile_tokens.insert(t.clone(), profile_id.to_owned());
            t
        });
        Ok(ProfileVerification {
            valid: true,
            profile_token: Some(Secret::new(token)),
            expires_at: None,
        })
    }

    async fn list_user_libraries(&self, _origin: &str, auth: &Auth) -> Result<Vec<UserLibrary>> {
        self.check(auth)?;
        let profile = self.check_profile(auth)?;
        let allowed = profile.and_then(|p| self.restrictions.get(&p));
        Ok(self
            .libraries
            .iter()
            .filter(|l| allowed.is_none_or(|ids| ids.contains(&l.id)))
            .cloned()
            .collect())
    }

    async fn home_sections(&self, _origin: &str, auth: &Auth) -> Result<SectionCollection> {
        self.check(auth)?;
        let cards: Vec<CatalogItem> = self.visible_cards(auth)?.into_iter().map(|(_, c)| c).collect();
        let continuing: Vec<CatalogItem> = cards.iter().filter(|c| c.position_seconds.is_some()).cloned().collect();
        let section = |id: &str, title: &str, kind: &str, featured: bool, items: Vec<CatalogItem>| Section {
            id: id.into(),
            title: title.into(),
            section_type: kind.into(),
            featured,
            total_count: i64::try_from(items.len()).unwrap_or_default(),
            items,
            has_more: Some(false),
        };
        Ok(SectionCollection {
            sections: vec![
                section(
                    "featured",
                    "Featured",
                    "featured",
                    true,
                    cards.iter().take(1).cloned().collect(),
                ),
                section("continue", "Continue watching", "continue_watching", false, continuing),
                section("recent", "Recently added", "recently_added", false, cards),
            ],
        })
    }

    async fn catalog_browse(
        &self,
        _origin: &str,
        auth: &Auth,
        query: &CatalogQuery,
    ) -> Result<CatalogBrowseCollection> {
        self.check(auth)?;
        let needle = query.q.as_deref().map(str::to_lowercase);
        let mut items: Vec<CatalogItem> = self
            .visible_cards(auth)?
            .into_iter()
            .filter(|(lib, _)| query.library_id.as_deref().is_none_or(|l| l == *lib))
            .filter(|(_, c)| needle.as_deref().is_none_or(|n| c.title.to_lowercase().contains(n)))
            .map(|(_, c)| c)
            .collect();
        match query.sort.as_deref() {
            Some("-release_date") => items.sort_by_key(|c| std::cmp::Reverse(c.year)),
            _ => items.sort_by(|a, b| a.title.cmp(&b.title)),
        }
        // Cursor = offset as a string (opaque to the client).
        let start = query
            .cursor
            .as_deref()
            .and_then(|c| c.parse::<usize>().ok())
            .unwrap_or(0);
        let limit = query
            .limit
            .and_then(|l| usize::try_from(l).ok())
            .filter(|l| *l > 0)
            .unwrap_or(60);
        let total = i64::try_from(items.len()).unwrap_or_default();
        let end = (start + limit).min(items.len());
        let page: Vec<CatalogItem> = items.get(start..end).map(<[CatalogItem]>::to_vec).unwrap_or_default();
        Ok(CatalogBrowseCollection {
            items: page,
            page: Some(PageInfo {
                has_more: end < items.len(),
                next_cursor: (end < items.len()).then(|| end.to_string()),
            }),
            total,
            total_exact: true,
        })
    }

    async fn catalog_item(&self, _origin: &str, auth: &Auth, id: &str) -> Result<CatalogItemDetail> {
        self.check(auth)?;
        self.check_profile(auth)?;
        demo::detail(id).ok_or_else(|| problem(404, "not_found"))
    }

    async fn series_seasons(&self, _origin: &str, auth: &Auth, series_id: &str) -> Result<SeasonCollection> {
        self.check(auth)?;
        self.check_profile(auth)?;
        if series_id != "s1" {
            return Err(problem(404, "not_found"));
        }
        Ok(SeasonCollection {
            items: vec![Season {
                content_id: "s1-1".into(),
                title: "Season 1".into(),
                season_number: 1,
                episode_count: 3,
                poster_url: demo::art("poster", "s1"),
                play_content_id: Some("e2".into()),
                ..Season::default()
            }],
            page: None,
        })
    }

    async fn season_episodes(
        &self,
        _origin: &str,
        auth: &Auth,
        series_id: &str,
        season: i64,
    ) -> Result<EpisodeCollection> {
        self.check(auth)?;
        self.check_profile(auth)?;
        if series_id != "s1" || season != 1 {
            return Err(problem(404, "not_found"));
        }
        Ok(EpisodeCollection {
            items: demo::episodes(),
            page: None,
        })
    }

    async fn watch_state(&self, _origin: &str, auth: &Auth, id: &str) -> Result<WatchDetail> {
        self.check(auth)?;
        self.check_profile(auth)?;
        let d = demo::detail(id)
            .filter(|d| !d.versions.is_empty())
            .ok_or_else(|| problem(404, "not_found"))?;
        Ok(WatchDetail {
            content_id: d.content_id,
            title: d.title,
            kind: d.kind,
            series_title: d.series_title,
            season_number: d.season_number,
            episode_number: d.episode_number,
            versions: d.versions,
            user_data: d.user_data,
            effective_subtitle_language: None,
            effective_subtitle_mode: Some("auto".into()),
            ..WatchDetail::default()
        })
    }

    async fn fetch_image(&self, url: &str) -> Result<ImageData> {
        let key = url
            .split("/api/v2/artwork/")
            .nth(1)
            .and_then(|rest| rest.split('?').next())
            .ok_or_else(|| problem(404, "not_found"))?;
        Ok(ImageData {
            content_type: "image/png".into(),
            bytes: demo::png(key),
        })
    }
}

/// Minimal PNG encoder for the mock placeholder artwork (8-bit RGB, stored deflate blocks, no dependencies).
mod png {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + u32::from(x)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        // Placeholder chunks are a few bytes, far below u32::MAX.
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }

    /// `raw` holds the scanlines, each prefixed with its filter byte; it must be shorter than 64 KiB.
    pub fn encode_rgb(width: u32, height: u32, raw: &[u8]) -> Vec<u8> {
        let len = u16::try_from(raw.len()).unwrap_or(0);
        let mut zlib = vec![0x78, 0x01, 0x01];
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(&raw[..usize::from(len)]);
        zlib.extend_from_slice(&adler32(&raw[..usize::from(len)]).to_be_bytes());
        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut out, b"IHDR", &ihdr);
        chunk(&mut out, b"IDAT", &zlib);
        chunk(&mut out, b"IEND", &[]);
        out
    }

    #[cfg(test)]
    #[test]
    fn crc_matches_the_png_spec_vector() {
        // CRC of "IEND" with no data is the well-known AE426082.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }
}
