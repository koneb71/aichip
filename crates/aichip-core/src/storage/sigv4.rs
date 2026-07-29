//! AWS Signature V4, enough of it to talk to MinIO.
//!
//! Hand-written rather than delegated to an SDK. The request surface is four
//! verbs against one known endpoint, and every S3 SDK also brings credential
//! *discovery* — reading `~/.aws`, environment chains, instance metadata.
//! Wandering off to find credentials nobody handed us is the opposite of what
//! this project promises, so the credentials here are passed in explicitly and
//! nothing else is consulted.
//!
//! Scope: `UNSIGNED-PAYLOAD` is never used; every request signs its body hash,
//! which is what MinIO expects and what makes a tampered upload fail loudly.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Everything a signature needs that isn't the request itself.
pub struct Credentials<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub region: &'a str,
}

/// One request to sign.
pub struct Request<'a> {
    pub method: &'a str,
    /// Already-encoded path, beginning with `/`, e.g. `/bucket/some%20key`.
    pub path: &'a str,
    /// Canonical (sorted, encoded) query string; empty when there is none.
    pub query: &'a str,
    pub host: &'a str,
    pub payload: &'a [u8],
    /// `YYYYMMDDTHHMMSSZ`, passed in so signing is a pure function.
    pub timestamp: &'a str,
}

/// The headers to add to the request: `x-amz-date`, `x-amz-content-sha256`,
/// `authorization`, in that order.
pub fn sign(req: &Request<'_>, creds: &Credentials<'_>) -> Vec<(String, String)> {
    let date = &req.timestamp[..8];
    let payload_hash = sha256_hex(req.payload);

    // Signed headers, sorted by name — the ordering is part of the signature,
    // not a stylistic choice.
    let canonical_headers = format!(
        "host:{}\nx-amz-content-sha256:{}\nx-amz-date:{}\n",
        req.host, payload_hash, req.timestamp
    );
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        req.method, req.path, req.query, canonical_headers, signed_headers, payload_hash
    );

    let scope = format!("{}/{}/s3/aws4_request", date, creds.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        req.timestamp,
        scope,
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac(format!("AWS4{}", creds.secret_key).as_bytes(), date);
    let k_region = hmac(&k_date, creds.region);
    let k_service = hmac(&k_region, "s3");
    let k_signing = hmac(&k_service, "aws4_request");
    let signature = hex::encode(hmac(&k_signing, &string_to_sign));

    vec![
        ("x-amz-date".into(), req.timestamp.to_string()),
        ("x-amz-content-sha256".into(), payload_hash),
        (
            "authorization".into(),
            format!(
                "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
                creds.access_key, scope, signed_headers, signature
            ),
        ),
    ]
}

/// Percent-encode one path segment.
///
/// S3 wants RFC 3986 encoding, which is *not* what form encoding does: a space
/// must become `%20` and never `+`, or the signature covers a different path
/// than the one the server routes.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_is_stable_for_the_same_inputs() {
        let creds = Credentials {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            region: "us-east-1",
        };
        let req = Request {
            method: "GET",
            path: "/bucket/key",
            query: "",
            host: "example.com",
            payload: b"",
            timestamp: "20150830T123600Z",
        };
        let a = sign(&req, &creds);
        let b = sign(&req, &creds);
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
        assert!(a[2].1.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/s3/"));
    }

    /// Changing anything the signature covers has to change the signature —
    /// otherwise a request could be rewritten in flight and still verify.
    #[test]
    fn every_signed_field_actually_affects_the_signature() {
        let creds = Credentials {
            access_key: "AKID",
            secret_key: "SECRET",
            region: "us-east-1",
        };
        let base = Request {
            method: "PUT",
            path: "/b/k",
            query: "",
            host: "h",
            payload: b"body",
            timestamp: "20260101T000000Z",
        };
        let sig = |r: &Request<'_>| sign(r, &creds)[2].1.clone();
        let original = sig(&base);

        let variants = [
            Request { method: "GET", ..copy(&base) },
            Request { path: "/b/other", ..copy(&base) },
            Request { query: "x=1", ..copy(&base) },
            Request { host: "elsewhere", ..copy(&base) },
            Request { payload: b"tampered", ..copy(&base) },
            Request { timestamp: "20260102T000000Z", ..copy(&base) },
        ];
        for v in &variants {
            assert_ne!(sig(v), original, "a signed field did not affect the signature");
        }
    }

    fn copy<'a>(r: &Request<'a>) -> Request<'a> {
        Request {
            method: r.method,
            path: r.path,
            query: r.query,
            host: r.host,
            payload: r.payload,
            timestamp: r.timestamp,
        }
    }

    /// A space must be `%20`. Form encoding's `+` would sign a different path
    /// than the one the server routes, and the failure is a 403 that looks
    /// like bad credentials.
    #[test]
    fn spaces_encode_as_percent_twenty_not_plus() {
        assert_eq!(encode_segment("my file.png"), "my%20file.png");
        assert!(!encode_segment("a b").contains('+'));
    }

    #[test]
    fn unreserved_characters_are_left_alone() {
        assert_eq!(encode_segment("a-Z_0.9~"), "a-Z_0.9~");
        // Slashes are encoded, because this encodes one *segment*; the caller
        // joins them. Encoding a whole path here would flatten the key.
        assert_eq!(encode_segment("a/b"), "a%2Fb");
    }
}
