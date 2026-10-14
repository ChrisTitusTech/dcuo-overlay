//! Bounded API reference lookup, independent of live memory observations.
//!
//! Contract: dcuobot-api CharacterApi and CharacterResponse DTOs. Transport bounds
//! use ureq 3.4.2's global timeout and decoded body limit. Names compare exactly
//! except ASCII letter case; world IDs must match exactly. API stats are maxima,
//! never current resource readings. The API provides no reference freshness time.

use serde::Deserialize;
use std::time::Duration;
use url::Url;

pub const DEFAULT_BASE_URL: &str = "https://dcuo.bot/api";
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct CharacterReference {
    pub id: String,
    pub name: String,
    pub world_id: String,
    pub power: Option<String>,
    pub movement: Option<String>,
    pub alignment: Option<String>,
    pub combat_rating: Option<u32>,
    pub skill_points: Option<u32>,
    pub health: Option<u64>,
    pub power_max: Option<u64>,
    pub league: Option<String>,
}

#[derive(Deserialize)]
struct Response {
    character_id: String,
    name: String,
    world_id: String,
    power_type: Option<String>,
    movement_mode: Option<String>,
    alignment: Option<String>,
    combat_rating: Option<u32>,
    skill_points: Option<u32>,
    stats: Option<Stats>,
    guild: Option<Guild>,
}

#[derive(Deserialize)]
struct Stats {
    health: Option<u64>,
    power: Option<u64>,
}

#[derive(Deserialize)]
struct Guild {
    name: Option<String>,
}

fn request_url(base_url: &str, name: &str, world: &str) -> Result<Url, String> {
    // The service deliberately switches to wildcard search below 3 characters.
    if name.chars().count() < 3
        || name.chars().count() > 128
        || name.trim() != name
        || name.chars().any(|c| c.is_control() || "*?^$\\".contains(c))
    {
        return Err("Enter an exact character name of 3 to 128 characters.".into());
    }
    if !matches!(world, "2" | "4" | "10" | "11" | "5001") {
        return Err("Select a supported world ID.".into());
    }
    let mut url = Url::parse(base_url).map_err(|_| "Invalid API base URL.".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid API base URL.".into());
    }
    let path = format!("{}/v1/census/characters", url.path().trim_end_matches('/'));
    url.set_path(&path);
    url.query_pairs_mut()
        .append_pair("name", name)
        .append_pair("worldId", world);
    Ok(url)
}

fn decode(bytes: &[u8], name: &str, world: &str) -> Result<CharacterReference, String> {
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("API response exceeds the size limit.".into());
    }
    let response: Response = serde_json::from_slice(bytes)
        .map_err(|_| "API returned an invalid character response.".to_string())?;
    if response.character_id.is_empty()
        || !response.character_id.bytes().all(|c| c.is_ascii_digit())
        || !response.name.eq_ignore_ascii_case(name)
        || response.world_id != world
    {
        return Err("API character identity does not match the requested name and world.".into());
    }
    Ok(CharacterReference {
        id: response.character_id,
        name: response.name,
        world_id: response.world_id,
        power: response.power_type,
        movement: response.movement_mode,
        alignment: response.alignment,
        combat_rating: response.combat_rating,
        skill_points: response.skill_points,
        health: response.stats.as_ref().and_then(|s| s.health),
        power_max: response.stats.and_then(|s| s.power),
        league: response.guild.and_then(|g| g.name),
    })
}

/// Fetch one exact API reference. Errors never echo URLs, names, or server bodies.
/// All redirects are refused; the complete request/body has an eight-second
/// global timeout and the decoded response body is limited to one MiB.
pub fn lookup(base_url: &str, name: &str, world: &str) -> Result<CharacterReference, String> {
    let url = request_url(base_url, name, world)?;
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .new_agent();
    let mut response = agent
        .get(url.as_str())
        .header(
            "User-Agent",
            concat!("DCUO-Companion/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/json")
        .call()
        .map_err(|_| "API request failed or timed out.".to_string())?;
    match response.status().as_u16() {
        200 => {}
        404 => return Err("No API character was found for that exact name and world.".into()),
        429 => return Err("API rate limit reached; try again later.".into()),
        300..=399 => return Err("API redirects are not accepted.".into()),
        _ => return Err("API returned an unsuccessful status.".into()),
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|_| "API response failed, timed out, or exceeded the size limit.".to_string())?;
    decode(&bytes, name, world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve_once(
        status: &str,
        headers: &str,
        body: Vec<u8>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response_headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
            body.len()
        );
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0 && request.len() < 8192);
                request.extend_from_slice(&chunk[..count]);
            }
            stream.write_all(response_headers.as_bytes()).unwrap();
            // A limited reader may close before accepting the entire body.
            let _ = stream.write_all(&body);
        });
        (url, thread)
    }

    #[test]
    fn fields_are_typed_and_maxima_are_preserved() {
        let value = decode(br#"{"character_id":"123","name":"Synthetic","world_id":"2","power_type":"Fire","movement_mode":"Flight","alignment":"Hero","combat_rating":450,"skill_points":600,"stats":{"health":100000,"power":50000},"guild":{"name":"Example"}}"#, "synthetic", "2").unwrap();
        assert_eq!(value.id, "123");
        assert_eq!(value.power.as_deref(), Some("Fire"));
        assert_eq!(value.movement.as_deref(), Some("Flight"));
        assert_eq!(value.alignment.as_deref(), Some("Hero"));
        assert_eq!(value.combat_rating, Some(450));
        assert_eq!(value.skill_points, Some(600));
        assert_eq!(value.health, Some(100000));
        assert_eq!(value.power_max, Some(50000));
        assert_eq!(value.league.as_deref(), Some("Example"));
    }

    #[test]
    fn missing_and_null_fields_remain_unknown() {
        for extra in [
            "",
            r#", "stats":null,"guild":null,"combat_rating":null,"power_type":null"#,
            r#", "stats":{"health":null},"guild":{}"#,
        ] {
            let json =
                format!(r#"{{"character_id":"123","name":"Synthetic","world_id":"2"{extra}}}"#);
            let value = decode(json.as_bytes(), "Synthetic", "2").unwrap();
            assert!(value.health.is_none() && value.power_max.is_none());
            assert!(value.combat_rating.is_none() && value.skill_points.is_none());
            assert!(value.power.is_none() && value.league.is_none());
        }
    }

    #[test]
    fn rejects_wrong_missing_ambiguous_or_malformed_identity() {
        for json in [
            r#"{"character_id":"123","name":"SyntheticOther","world_id":"2"}"#,
            r#"{"character_id":"123","name":"Synthetic","world_id":"4"}"#,
            r#"{"character_id":"12*","name":"Synthetic","world_id":"2"}"#,
            r#"{"character_id":"","name":"Synthetic","world_id":"2"}"#,
            r#"{"name":"Synthetic","world_id":"2"}"#,
            r#"[{"character_id":"123","name":"Synthetic","world_id":"2"}]"#,
            r#"{"character_id":"123","name":"Synthetic","world_id":"2","combat_rating":-1}"#,
        ] {
            assert!(decode(json.as_bytes(), "Synthetic", "2").is_err());
        }
    }

    #[test]
    fn rejects_oversized_response() {
        assert!(
            decode(
                &vec![b' '; MAX_RESPONSE_BYTES as usize + 1],
                "Synthetic",
                "2"
            )
            .is_err()
        );
    }

    #[test]
    fn transport_accepts_exact_reference_and_rejects_oversize() {
        let (url, server) = serve_once(
            "200 OK",
            "",
            br#"{"character_id":"123","name":"Synthetic","world_id":"2"}"#.to_vec(),
        );
        assert_eq!(lookup(&url, "Synthetic", "2").unwrap().id, "123");
        server.join().unwrap();
        let (url, server) = serve_once("200 OK", "", vec![b' '; MAX_RESPONSE_BYTES as usize + 1]);
        assert!(
            lookup(&url, "Synthetic", "2")
                .unwrap_err()
                .contains("size limit")
        );
        server.join().unwrap();
    }

    #[test]
    fn transport_errors_are_sanitized_and_redirects_refused() {
        for (status, headers, expected) in [
            (
                "302 Found",
                "Location: https://unknown.example/private\r\n",
                "redirects",
            ),
            ("404 Not Found", "", "No API character"),
            ("429 Too Many Requests", "", "rate limit"),
            ("500 Server Error", "", "unsuccessful status"),
            ("200 OK", "", "invalid character response"),
        ] {
            let (url, server) = serve_once(status, headers, b"private server details".to_vec());
            let error = lookup(&url, "Synthetic", "2").unwrap_err();
            assert!(error.contains(expected));
            assert!(!error.contains("private") && !error.contains(&url));
            server.join().unwrap();
        }
    }

    #[test]
    fn network_failure_does_not_echo_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(
            lookup(&url, "Synthetic", "2").unwrap_err(),
            "API request failed or timed out."
        );
    }

    #[test]
    fn encodes_name_and_world_as_query_values() {
        let url = request_url(DEFAULT_BASE_URL, "Synthetic & Test", "2").unwrap();
        let pairs: Vec<_> = url.query_pairs().collect();
        assert_eq!(url.path(), "/api/v1/census/characters");
        assert_eq!(pairs[0], ("name".into(), "Synthetic & Test".into()));
        assert_eq!(pairs[1], ("worldId".into(), "2".into()));
        for (base, name, world) in [
            ("https://user:secret@example.test", "Synthetic", "2"),
            (DEFAULT_BASE_URL, "Sy", "2"),
            (DEFAULT_BASE_URL, "Synthetic*", "2"),
            (DEFAULT_BASE_URL, "Synthetic", "*"),
        ] {
            assert!(request_url(base, name, world).is_err());
        }
    }
}
