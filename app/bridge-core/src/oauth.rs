//! 内嵌迷你 OAuth 2.1 授权服务器(按 OpenAI plugins/build/auth 与 MCP 授权规范实现)
//!
//! - DCR 动态客户端注册(RFC 7591):ChatGPT 每连接注册一次
//! - 授权码流程 + PKCE(S256,must)
//! - access token 1h,refresh token 30d 且每次刷新轮换(public client MUST)
//! - resource(RFC 8707)透传并绑定 aud;授权响应带 iss(RFC 9207)
//! - 内存存储:重启后令牌失效,ChatGPT 收到 401 会自动重走授权流程

use base64::Engine;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

const ACCESS_TTL: Duration = Duration::from_secs(3600);
const REFRESH_TTL: Duration = Duration::from_secs(30 * 86400);
const CODE_TTL: Duration = Duration::from_secs(300);

#[derive(Clone)]
struct CodeGrant {
    client_id: String,
    redirect_uri: String,
    scope: String,
    code_challenge: String,
    resource: Option<String>,
    expires: Instant,
}

#[derive(Clone)]
pub struct TokenInfo {
    #[allow(dead_code)]
    pub client_id: String,
    #[allow(dead_code)]
    pub scope: String,
    pub resource: Option<String>,
    pub expires: Instant,
}

#[derive(Clone)]
struct RefreshInfo {
    client_id: String,
    scope: String,
    resource: Option<String>,
    expires: Instant,
}

/// 待确认的授权请求(网页输码后放行)
#[derive(Clone)]
pub struct PendingAuth {
    pub query: HashMap<String, String>,
    pub code: String,
    pub expires: Instant,
}

const PAIRING_TTL: Duration = Duration::from_secs(120);
const PAIRING_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789"; // 去掉易混淆 0O1IL

/// 生成 XXXX-XXXX 8 位配对码
pub fn gen_pairing_code() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    let chars: Vec<char> = b.iter().map(|x| PAIRING_ALPHABET[(*x as usize) % PAIRING_ALPHABET.len()] as char).collect();
    format!("{}{}{}{}-{}{}{}{}", chars[0], chars[1], chars[2], chars[3], chars[4], chars[5], chars[6], chars[7])
}

pub struct OAuthServer {
    clients: RwLock<HashMap<String, Vec<String>>>,
    codes: RwLock<HashMap<String, CodeGrant>>,
    access: RwLock<HashMap<String, TokenInfo>>,
    refresh: RwLock<HashMap<String, RefreshInfo>>,
    pending: RwLock<HashMap<String, PendingAuth>>,
}

fn rand_token(prefix: &str) -> String {
    use rand::RngCore;
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    let hex: String = b.iter().map(|x| format!("{:02x}", x)).collect();
    format!("{}_{}", prefix, hex)
}

impl Default for OAuthServer {
    fn default() -> Self {
        Self::new()
    }
}

impl OAuthServer {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
            codes: RwLock::new(HashMap::new()),
            access: RwLock::new(HashMap::new()),
            refresh: RwLock::new(HashMap::new()),
            pending: RwLock::new(HashMap::new()),
        }
    }

    /// 登记一笔待配对授权,返回配对码(软件弹窗展示)
    pub fn create_pending(&self, query: HashMap<String, String>) -> String {
        let code = gen_pairing_code();
        self.pending.write().unwrap().insert(
            code.clone(),
            PendingAuth { query, code: code.clone(), expires: Instant::now() + PAIRING_TTL },
        );
        code
    }

    /// 网页提交配对码:有效则取出授权请求(一次性),过期/不存在返回 None
    pub fn take_pending(&self, code: &str) -> Option<PendingAuth> {
        let norm = code.trim().to_uppercase();
        let p = self.pending.write().unwrap().remove(&norm)?;
        if Instant::now() > p.expires {
            return None;
        }
        Some(p)
    }

    /// 拒绝时按码清理
    pub fn drop_pending(&self, code: &str) {
        self.pending.write().unwrap().remove(&code.trim().to_uppercase());
    }

    /* ── DCR(RFC 7591) ── */
    pub fn register(&self, redirect_uris: Vec<String>) -> Value {
        let client_id = rand_token("mcpb_client");
        self.clients.write().unwrap().insert(client_id.clone(), redirect_uris.clone());
        json!({
            "client_id": client_id,
            "client_id_issued_at": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            "redirect_uris": redirect_uris,
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
            "client_secret_expires_at": 0
        })
    }

    fn client_redirect_ok(&self, client_id: &str, redirect_uri: &str) -> bool {
        self.clients
            .read()
            .unwrap()
            .get(client_id)
            .map(|uris| uris.iter().any(|u| u == redirect_uri))
            .unwrap_or(false)
    }

    /// 校验 /authorize 请求;Ok 返回规范化的参数集
    pub fn check_authorize(&self, q: &HashMap<String, String>) -> Result<(), String> {
        if q.get("response_type").map(String::as_str) != Some("code") {
            return Err("response_type 必须为 code".into());
        }
        let client_id = q.get("client_id").ok_or("缺少 client_id")?;
        let redirect_uri = q.get("redirect_uri").ok_or("缺少 redirect_uri")?;
        if !self.client_redirect_ok(client_id, redirect_uri) {
            return Err("client_id 未注册或 redirect_uri 不匹配".into());
        }
        // MCP 规范:必须 PKCE S256
        if q.get("code_challenge_method").map(String::as_str) != Some("S256")
            || q.get("code_challenge").map(|s| s.is_empty()).unwrap_or(true)
        {
            return Err("需要 PKCE(S256 code_challenge)".into());
        }
        Ok(())
    }

    /// 用户点「允许」:签发授权码并拼 302 地址(带 state + iss)
    pub fn approve(&self, q: &HashMap<String, String>, issuer: &str) -> String {
        let code = rand_token("mcpb_code");
        self.codes.write().unwrap().insert(
            code.clone(),
            CodeGrant {
                client_id: q["client_id"].clone(),
                redirect_uri: q["redirect_uri"].clone(),
                scope: q.get("scope").cloned().unwrap_or_else(|| "mcp".into()),
                code_challenge: q["code_challenge"].clone(),
                resource: q.get("resource").cloned(),
                expires: Instant::now() + CODE_TTL,
            },
        );
        self.redirect_with(q, &[("code", &code), ("iss", issuer)])
    }

    /// 用户点「拒绝」
    pub fn deny(&self, q: &HashMap<String, String>, issuer: &str) -> String {
        self.redirect_with(q, &[("error", "access_denied"), ("iss", issuer)])
    }

    fn redirect_with(&self, q: &HashMap<String, String>, extra: &[(&str, &str)]) -> String {
        let mut url = q["redirect_uri"].clone();
        url.push(if url.contains('?') { '&' } else { '?' });
        let mut parts: Vec<String> = extra.iter().map(|(k, v)| format!("{}={}", k, urlencode(v))).collect();
        if let Some(state) = q.get("state") {
            parts.push(format!("state={}", urlencode(state)));
        }
        url + &parts.join("&")
    }

    /* ── /token:authorization_code 与 refresh_token 两种 grant ── */
    pub fn token(&self, form: &HashMap<String, String>) -> Result<Value, (u16, Value)> {
        match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => self.grant_code(form),
            Some("refresh_token") => self.grant_refresh(form),
            _ => Err((400, json!({"error": "unsupported_grant_type"}))),
        }
    }

    fn grant_code(&self, form: &HashMap<String, String>) -> Result<Value, (u16, Value)> {
        let code = form.get("code").ok_or((400, json!({"error":"invalid_request"})))?;
        let grant = self.codes.write().unwrap().remove(code).ok_or((400, json!({"error":"invalid_grant"})))?;
        if Instant::now() > grant.expires {
            return Err((400, json!({"error":"invalid_grant","error_description":"code expired"})));
        }
        if form.get("client_id").map(String::as_str) != Some(grant.client_id.as_str())
            || form.get("redirect_uri").map(String::as_str) != Some(grant.redirect_uri.as_str())
        {
            return Err((400, json!({"error":"invalid_grant","error_description":"client/redirect mismatch"})));
        }
        // PKCE S256 校验
        let verifier = form.get("code_verifier").ok_or((400, json!({"error":"invalid_request","error_description":"missing code_verifier"})))?;
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        if challenge != grant.code_challenge {
            return Err((400, json!({"error":"invalid_grant","error_description":"PKCE verification failed"})));
        }
        Ok(self.issue(&grant.client_id, &grant.scope, grant.resource.clone(), None))
    }

    fn grant_refresh(&self, form: &HashMap<String, String>) -> Result<Value, (u16, Value)> {
        let rt = form.get("refresh_token").ok_or((400, json!({"error":"invalid_request"})))?;
        // 轮换:旧 refresh token 立即作废(public client MUST)
        let info = self.refresh.write().unwrap().remove(rt).ok_or((400, json!({"error":"invalid_grant"})))?;
        if Instant::now() > info.expires {
            return Err((400, json!({"error":"invalid_grant","error_description":"refresh token expired"})));
        }
        Ok(self.issue(&info.client_id, &info.scope, info.resource.clone(), None))
    }

    fn issue(&self, client_id: &str, scope: &str, resource: Option<String>, _unused: Option<()>) -> Value {
        let access_token = rand_token("mcpb_at");
        let refresh_token = rand_token("mcpb_rt");
        self.access.write().unwrap().insert(
            access_token.clone(),
            TokenInfo {
                client_id: client_id.into(),
                scope: scope.into(),
                resource: resource.clone(),
                expires: Instant::now() + ACCESS_TTL,
            },
        );
        self.refresh.write().unwrap().insert(
            refresh_token.clone(),
            RefreshInfo {
                client_id: client_id.into(),
                scope: scope.into(),
                resource,
                expires: Instant::now() + REFRESH_TTL,
            },
        );
        json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": ACCESS_TTL.as_secs(),
            "refresh_token": refresh_token,
            "scope": scope
        })
    }

    /* ── 资源服务器侧校验:token 存在、未过期、aud 匹配 ── */
    /// aud 比较忽略 scheme(本地 http 回环 vs 公网 https 的差异)
    pub fn verify(&self, token: &str, accepted_resources: &[String]) -> bool {
        let store = self.access.read().unwrap();
        match store.get(token) {
            Some(info) if Instant::now() <= info.expires => match &info.resource {
                None => true,
                Some(aud) => {
                    let aud_n = strip_scheme(aud);
                    accepted_resources.iter().any(|r| strip_scheme(r) == aud_n)
                }
            },
            _ => false,
        }
    }

    /// 自检用:服务端直接铸一枚本地 token(不经过授权页)
    pub fn mint_local(&self, resource: Option<String>) -> String {
        let t = rand_token("mcpb_local");
        self.access.write().unwrap().insert(
            t.clone(),
            TokenInfo {
                client_id: "selfcheck".into(),
                scope: "mcp".into(),
                resource,
                expires: Instant::now() + ACCESS_TTL,
            },
        );
        t
    }
}

fn strip_scheme(u: &str) -> &str {
    u.strip_prefix("https://").or_else(|| u.strip_prefix("http://")).unwrap_or(u)
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}
