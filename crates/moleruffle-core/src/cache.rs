//! 本地资源缓存(CDN 思路):把摩尔庄园从 台版服务器流式拉取的**静态资源**
//! (几百个 SWF / 图片)缓存到磁盘。二次加载直接读本地——秒开、且不依赖网络,
//! 缓解台版资源服务器慢/抖动导致的资源拉取失败("可能服务器在维护")与每次重下的低效。
//!
//! 实现:包一层 [`CachingNavigator`] 在 `ExternalNavigatorBackend` 外面,只重写 `fetch`:
//!   - 可缓存(规则见 [`cache_policy`])→ 命中读盘、未命中走内层网络后存盘;
//!     只缓存 HTTP 200 且档头像资源档的回应。
//!   - 其余(登录 account.61.com / POST / socket)原样透传给内层,绝不缓存动态内容。
//!
//! socket(游戏服 123.206.131.236:1865 的实时协议)走 `connect_socket`,天然不经此缓存。

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use encoding_rs::Encoding;
use indexmap::IndexMap;
use ruffle_core::backend::navigator::{
    ErrorResponse, NavigationMethod, NavigatorBackend, OwnedFuture, Request, SuccessResponse,
};
use ruffle_core::loader::Error;
use ruffle_core::socket::{SocketAction, SocketHandle};
use url::{ParseError, Url};

/// 每个请求的流水号,方便在日志里把 start/end 两行对起来。
static NEXT_FETCH_ID: AtomicU64 = AtomicU64::new(1);

/// 请求从哪里拿到资料(记日志用)。
#[derive(Clone, Copy)]
enum FetchSource {
    /// 非 GET,原样透传。
    Passthrough,
    /// 银行补丁(本机文件)。
    BankPatch,
    /// 磁盘缓存命中。
    CacheHit,
    /// 可缓存但未命中,走网络后写入缓存。
    CacheMiss,
    /// 不可缓存的 GET,走网络(含重试)。
    Network,
}

impl FetchSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::BankPatch => "bank_patch",
            Self::CacheHit => "cache_hit",
            Self::CacheMiss => "cache_miss",
            Self::Network => "network",
        }
    }
}

/// 失败重试次数(总尝试 = RETRIES + 1)。只对幂等 GET 生效。抖动网络下瞬时超时重试一次往往就成。
const RETRIES: u32 = 2;

/// 给 `ExternalNavigatorBackend` 套上本地资源缓存 + GET 重试。
/// 内层包 `Rc<RefCell<N>>`:重试需要在异步过程里重新发起请求,借此绕开 `&self` 的生命周期约束
/// (借用只在同步的 `fetch()`/`borrow_mut()` 调用期间持有,绝不跨 await)。
pub struct CachingNavigator<N> {
    inner: Rc<RefCell<N>>,
    cache_dir: PathBuf,
}

impl<N> CachingNavigator<N> {
    /// `cache_dir` 用各端缓存目录(iOS=沙盒 Library/Caches,可被系统按需清理,正合缓存语义)。
    pub fn new(inner: N, cache_dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        tracing::info!("资源缓存目录: {}", cache_dir.display());
        Self {
            inner: Rc::new(RefCell::new(inner)),
            cache_dir,
        }
    }

    /// URL → 缓存文件路径(对完整 URL 取 FNV-1a hash,按前两位分桶,避免单目录文件过多)。
    fn cache_path(&self, url: &str) -> PathBuf {
        let hex = format!("{:016x}", fnv1a64(url.as_bytes()));
        self.cache_dir.join(&hex[0..2]).join(format!("{hex}.swfcache"))
    }
}

/// 快取规则(一律对 resolve 后的绝对 URL 判断;游戏常用相对路径,直接 parse 会失败而漏掉):
/// - 只缓存 GET、无 body、台版资源 host、静态资源扩展名(SWF/图片/MP3)。
/// - 入口 `Client.swf` 不缓存:服务器更新时必须拿到新版。
/// - 带查询字串时,只接受游戏版本管理器加的版本号(档案修改时间的 36 进位,如 `?i1hkoxuw`)。
///   档案一更新版本号就变,所以按完整 URL 永久缓存是安全的。带 `&`、`=` 或纯数字
///   (`&123` 之类的随机防快取参数)的一律不缓存。两个服务生动画例外,任何查询都可缓存。
/// - 没有版本号的资源只缓存 [`UNVERSIONED_TTL`],过期就重新下载,避免服务器更新后一直用旧档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CachePolicy {
    /// 不缓存。
    None,
    /// URL 带版本号,永久有效。
    Versioned,
    /// 没有版本号,只在 [`UNVERSIONED_TTL`] 内有效。
    Unversioned,
}

/// 没有版本号的资源在缓存里能用多久。够一次长时间游玩反复载入,又不至于一直用旧档。
const UNVERSIONED_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn is_restaurant_animation(url: &Url) -> bool {
    url.host_str() == Some("mole.61.com.tw")
        && matches!(url.path(), "/resource/petcloth/swf2/1200042.swf" | "/resource/petcloth/swf2/1200043.swf")
}

/// 游戏版本管理器(`TaomeeVersionManager.getVerURLByNameSpace`)加的版本号:
/// 档案修改时间 `toString(36)`,目前是 8 个小写英数字。
fn is_version_token(query: &str) -> bool {
    (5..=12).contains(&query.len())
        && query.bytes().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
        && query.bytes().any(|c| c.is_ascii_lowercase())
}

fn cache_policy(method: NavigationMethod, has_body: bool, url: &Url) -> CachePolicy {
    if method != NavigationMethod::Get || has_body {
        return CachePolicy::None;
    }
    let static_host = matches!(url.host_str(), Some("mole.61.com.tw" | "rsyncgametest.61.com.tw"));
    let path = url.path().to_ascii_lowercase();
    let static_asset = [".swf", ".png", ".jpg", ".jpeg", ".gif", ".mp3"]
        .iter()
        .any(|extension| path.ends_with(extension));
    if !static_host || !static_asset || path.ends_with("/client.swf") {
        return CachePolicy::None;
    }
    match url.query() {
        None => CachePolicy::Unversioned,
        Some(query) if is_version_token(query) || is_restaurant_animation(url) => CachePolicy::Versioned,
        Some(_) => CachePolicy::None,
    }
}

/// 写入快取前检查档头,避免把服务器维护时回 200 的 HTML 错误页永久缓存。
fn looks_like_asset(bytes: &[u8]) -> bool {
    const MAGIC: &[&[u8]] = &[
        b"FWS", b"CWS", b"ZWS", // SWF
        b"\x89PNG", b"\xFF\xD8\xFF", b"GIF8", // PNG / JPEG / GIF
        b"ID3", b"\xFF\xFB", b"\xFF\xF3", b"\xFF\xF2", b"\xFF\xFA", // MP3
    ];
    MAGIC.iter().any(|magic| bytes.starts_with(magic))
}

/// 缓存档是否还能用(没版本号的资源看修改时间是否超过 TTL)。
fn cache_entry_fresh(path: &std::path::Path, policy: CachePolicy) -> bool {
    match policy {
        CachePolicy::None => false,
        CachePolicy::Versioned => true,
        CachePolicy::Unversioned => std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age < UNVERSIONED_TTL),
    }
}

/// FNV-1a 64 位元。跟 `DefaultHasher` 不同,演算法固定,升级 Rust 后缓存路径不会变。
fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn write_cache_atomic(path: &PathBuf, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 写临时文件再 rename,保证不会出现半截的损坏缓存
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

impl<N: NavigatorBackend> CachingNavigator<N> {
    fn fetch_original(
        &self,
        request: Request,
    ) -> (FetchSource, OwnedFuture<Box<dyn SuccessResponse>, ErrorResponse>) {
        // 非 GET(POST 等)不幂等:既不缓存也不重试,原样透传(重试 POST 会重复提交)。
        if request.method() != NavigationMethod::Get {
            return (FetchSource::Passthrough, self.inner.borrow().fetch(request));
        }

        // Opt-in local compatibility patch for the six bank game buttons only.
        // Resolve relative requests; accept no other host, asset or HTTP method.
        if let Ok(resolved) = self.inner.borrow().resolve_url(request.url()) {
            if resolved.host_str() == Some("mole.61.com.tw")
                && resolved.path() == "/resource/map/69.swf"
            {
                if let Some(path) = std::env::var_os("MOLERUFFLE_BANK_SWF") {
                    match std::fs::read(path) {
                        Ok(bytes) if bytes.starts_with(b"CWS") || bytes.starts_with(b"FWS") => {
                            tracing::info!("Using bank trace compatibility patch");
                            return (FetchSource::BankPatch, Box::pin(async move {
                                Ok(Box::new(CachedResponse::new(resolved.to_string(), bytes))
                                    as Box<dyn SuccessResponse>)
                            }));
                        }
                        _ => tracing::warn!("Bank patch unavailable; using original server asset"),
                    }
                }
            }
        }

        // 一律用 resolve 后的绝对 URL 判断与当缓存键(游戏常给相对路径)。
        let resolved = self.inner.borrow().resolve_url(request.url()).ok();
        let url = resolved.as_ref().map_or_else(|| request.url().to_string(), Url::to_string);
        let policy = resolved.as_ref().map_or(CachePolicy::None, |resolved| {
            cache_policy(request.method(), request.body().is_some(), resolved)
        });
        let restaurant_animation = resolved.as_ref().is_some_and(is_restaurant_animation);
        let cache_path = (policy != CachePolicy::None).then(|| self.cache_path(&url));

        // 缓存命中:直接读盘返回(免网络,秒开)
        if let Some(path) = &cache_path {
            if cache_entry_fresh(path, policy) {
                if let Ok(bytes) = std::fs::read(path) {
                    if restaurant_animation {
                        tracing::info!("Restaurant animation cache hit: {url}");
                    }
                    let u = url.clone();
                    return (FetchSource::CacheHit, Box::pin(async move {
                        Ok(Box::new(CachedResponse::new(u, bytes)) as Box<dyn SuccessResponse>)
                    }));
                }
            }
        }

        // 未命中:走网络。GET 幂等,失败/超时重试 RETRIES 次;成功且可缓存(200)则存盘。
        let source = if cache_path.is_some() { FetchSource::CacheMiss } else { FetchSource::Network };
        let inner = self.inner.clone();
        let headers = request.headers().clone();
        (source, Box::pin(async move {
            let mut attempt = 0u32;
            loop {
                // 每次尝试重建一个 GET 请求(原请求已被上一次 fetch 消费)
                let mut req = Request::get(url.clone());
                req.set_headers(headers.clone());
                // 借用只在同步的 fetch() 调用期间持有,拿到 future 后立刻释放,绝不跨 await
                let started = Instant::now();
                let fut = inner.borrow().fetch(req);
                match fut.await {
                    Ok(resp) => {
                        let Some(path) = &cache_path else {
                            return Ok(resp); // 可重试但不缓存(如 account.61.com 的 GET)
                        };
                        if resp.status() != 200 {
                            return Ok(resp); // 非 200 不缓存,原样返回
                        }
                        let final_url = resp.url().to_string();
                        match resp.body().await {
                            Ok(bytes) => {
                                if restaurant_animation {
                                    tracing::info!(request_ms = started.elapsed().as_millis() as u64,
                                        bytes = bytes.len(), "Restaurant animation downloaded: {url}");
                                }
                                if looks_like_asset(&bytes) {
                                    write_cache_atomic(path, &bytes);
                                } else {
                                    // 例:version/version*.swf 其实是游戏自订的压缩格式,每次启动都会碰到。
                                    tracing::info!("回应不像资源档,不写入缓存: {url}");
                                }
                                return Ok(Box::new(CachedResponse::new(final_url, bytes))
                                    as Box<dyn SuccessResponse>);
                            }
                            Err(error) => {
                                if attempt < RETRIES {
                                    attempt += 1;
                                    tracing::debug!("读 body 失败,重试 {attempt}/{RETRIES}: {url}");
                                    continue;
                                }
                                return Err(ErrorResponse { url, error });
                            }
                        }
                    }
                    Err(err) => {
                        if attempt < RETRIES {
                            attempt += 1;
                            tracing::debug!("拉取失败,重试 {attempt}/{RETRIES}: {url}");
                            continue;
                        }
                        return Err(err);
                    }
                }
            }
        }))
    }

}

impl<N: NavigatorBackend> NavigatorBackend for CachingNavigator<N> {
    /// 每个请求记两行 info 日志(target `moleruffle_core::net`):
    /// `Fetch start`(发出时)与 `Fetch end`(拿到回应或失败时),以同一个 `id` 对应。
    /// `url` 是 resolve 后的完整网址;`bytes` 是回应声明的长度(网络串流时为 Content-Length)。
    /// 用来看每次卡顿/服务生消失前后到底在载什么、有没有命中缓存、两次载入是否重叠。
    fn fetch(&self, request: Request) -> OwnedFuture<Box<dyn SuccessResponse>, ErrorResponse> {
        let id = NEXT_FETCH_ID.fetch_add(1, Ordering::Relaxed);
        let resolved = self.inner.borrow().resolve_url(request.url());
        let url = resolved
            .as_ref()
            .map(|url| url.to_string())
            .unwrap_or_else(|_| request.url().to_string());
        let (source, future) = self.fetch_original(request);
        tracing::info!(target: "moleruffle_core::net", id, source = source.as_str(), "Fetch start {url}");
        let started = Instant::now();
        Box::pin(async move {
            let result = future.await;
            let elapsed_ms = started.elapsed().as_millis() as u64;
            match &result {
                Ok(response) => {
                    let bytes = response.expected_length().ok().flatten();
                    tracing::info!(target: "moleruffle_core::net", id, source = source.as_str(),
                        status = response.status(), elapsed_ms, bytes, "Fetch end {url}");
                }
                Err(error) => {
                    tracing::info!(target: "moleruffle_core::net", id, source = source.as_str(),
                        elapsed_ms, error = ?error.error, "Fetch failed {url}");
                }
            }
            result
        })
    }

    // ── 其余方法全部透传给内层(借用仅在调用期间)──
    fn navigate_to_url(
        &self,
        url: &str,
        target: &str,
        vars_method: Option<(NavigationMethod, IndexMap<String, String>)>,
    ) {
        self.inner.borrow().navigate_to_url(url, target, vars_method)
    }

    fn resolve_url(&self, url: &str) -> Result<Url, ParseError> {
        self.inner.borrow().resolve_url(url)
    }

    fn spawn_future(&mut self, future: OwnedFuture<(), Error>) {
        self.inner.borrow_mut().spawn_future(future)
    }

    fn pre_process_url(&self, url: Url) -> Url {
        self.inner.borrow().pre_process_url(url)
    }

    fn connect_socket(
        &mut self,
        host: String,
        port: u16,
        timeout: Duration,
        handle: SocketHandle,
        receiver: Receiver<Vec<u8>>,
        sender: Sender<SocketAction>,
    ) {
        self.inner
            .borrow_mut()
            .connect_socket(host, port, timeout, handle, receiver, sender)
    }
}

/// 从本地缓存字节合成的 `SuccessResponse`(模拟一次成功的 HTTP 200)。
/// 同时支持 `body`(整取)与 `next_chunk`(流式,一次给完)——两条加载路径都兼容。
struct CachedResponse {
    url: String,
    bytes: Vec<u8>,
    chunk_done: bool,
}

impl CachedResponse {
    fn new(url: String, bytes: Vec<u8>) -> Self {
        Self {
            url,
            bytes,
            chunk_done: false,
        }
    }
}

impl SuccessResponse for CachedResponse {
    fn url(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.url)
    }

    fn set_url(&mut self, url: String) {
        self.url = url;
    }

    fn body(self: Box<Self>) -> OwnedFuture<Vec<u8>, Error> {
        let bytes = self.bytes;
        Box::pin(async move { Ok(bytes) })
    }

    fn text_encoding(&self) -> Option<&'static Encoding> {
        None
    }

    fn status(&self) -> u16 {
        200
    }

    fn redirected(&self) -> bool {
        false
    }

    fn next_chunk(&mut self) -> OwnedFuture<Option<Vec<u8>>, Error> {
        if self.chunk_done {
            Box::pin(async { Ok(None) })
        } else {
            self.chunk_done = true;
            let bytes = std::mem::take(&mut self.bytes);
            Box::pin(async move { Ok(Some(bytes)) })
        }
    }

    fn expected_length(&self) -> Result<Option<u64>, Error> {
        Ok(Some(self.bytes.len() as u64))
    }
}

/// 资源缓存目录:各端缓存目录下 MoleRuffle/http(iOS=沙盒 Library/Caches/MoleRuffle/http)。
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MOLERUFFLE_DATA_DIR") {
        return PathBuf::from(dir).join("http");
    }
    dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("MoleRuffle")
        .join("http")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> CachePolicy {
        cache_policy(NavigationMethod::Get, false, &Url::parse(url).unwrap())
    }

    #[test]
    fn cache_excludes_sessions_entry_and_unrelated_hosts() {
        for url in [
            "http://mole.61.com.tw/Client.swf",
            "http://mole.61.com.tw/Client.swf?i1hkoxuw",
            "http://mole.61.com.tw/Client.swf?session=secret",
            "http://mole.61.com.tw/config/Server.xml",
            "http://mole.61.com.tw/version/zzz_config.txt",
            "http://mole.61.com.tw.example.org/resource/a.swf",
            "http://example.org/mole.61.com.tw/resource/a.swf",
            "http://mole.61.com/resource/a.swf",
        ] {
            assert_eq!(get(url), CachePolicy::None, "{url}");
        }
        let url = Url::parse("http://mole.61.com.tw/resource/a.swf").unwrap();
        assert_eq!(cache_policy(NavigationMethod::Post, false, &url), CachePolicy::None);
        assert_eq!(cache_policy(NavigationMethod::Get, true, &url), CachePolicy::None);
    }

    #[test]
    fn version_tokens_cache_permanently_random_queries_never() {
        for url in [
            "http://mole.61.com.tw/resource/farm/Animal/swf/1270027.swf?i1hkoxuw",
            "http://mole.61.com.tw/module/home/home.swf?i0zz9a1b",
            "http://mole.61.com.tw/resource/petcloth/swf2/1200042.swf?v=1",
            "http://mole.61.com.tw/resource/petcloth/swf2/1200043.swf?version",
        ] {
            assert_eq!(get(url), CachePolicy::Versioned, "{url}");
        }
        for url in [
            "http://mole.61.com.tw/resource/a.swf?i1hkoxuw&512",
            "http://mole.61.com.tw/resource/a.swf?512",
            "http://mole.61.com.tw/resource/a.swf?1700000000",
            "http://mole.61.com.tw/resource/a.swf?v=1",
            "http://mole.61.com.tw/resource/a.swf?I1HKOXUW",
            "http://mole.61.com.tw/resource/map/69.swf?v=1",
        ] {
            assert_eq!(get(url), CachePolicy::None, "{url}");
        }
    }

    #[test]
    fn unversioned_assets_cache_with_ttl() {
        for url in [
            "http://mole.61.com.tw/resource/restaurant/swf/1340012.swf",
            "http://rsyncgametest.61.com.tw/mole/resource/map.swf",
            "http://mole.61.com.tw/resource/logo.png",
        ] {
            assert_eq!(get(url), CachePolicy::Unversioned, "{url}");
        }
        let dir = std::env::temp_dir().join(format!("moleruffle-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("entry");
        assert!(!cache_entry_fresh(&path, CachePolicy::Unversioned), "missing file is stale");
        std::fs::write(&path, b"CWS").unwrap();
        assert!(cache_entry_fresh(&path, CachePolicy::Unversioned));
        assert!(cache_entry_fresh(&path, CachePolicy::Versioned));
        assert!(!cache_entry_fresh(&path, CachePolicy::None));
        let old = std::time::SystemTime::now() - UNVERSIONED_TTL - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(old).unwrap();
        assert!(!cache_entry_fresh(&path, CachePolicy::Unversioned), "expired");
        assert!(cache_entry_fresh(&path, CachePolicy::Versioned), "versioned never expires");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_real_assets_are_written() {
        for bytes in [&b"CWS\x0a"[..], b"FWS", b"ZWS", b"\x89PNG\r\n", b"\xFF\xD8\xFF\xE0", b"GIF89a", b"ID3\x03", b"\xFF\xFB\x90"] {
            assert!(looks_like_asset(bytes), "{bytes:?}");
        }
        for bytes in [&b"<html>maintenance"[..], b"<?xml", b"", b"CW"] {
            assert!(!looks_like_asset(bytes), "{bytes:?}");
        }
    }

    #[test]
    fn cache_key_is_stable_and_complete() {
        let cache = CachingNavigator { inner: Rc::new(RefCell::new(())), cache_dir: PathBuf::from("unused") };
        let a = "http://mole.61.com.tw/resource/petcloth/swf2/1200042.swf?v=1";
        let b = "http://mole.61.com.tw/resource/petcloth/swf2/1200042.swf?v=2";
        assert_ne!(cache.cache_path(a), cache.cache_path(b));
        // FNV-1a 的标准测试值,确保演算法固定不变。
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(
            cache.cache_path("http://mole.61.com.tw/a.swf"),
            PathBuf::from("unused").join(format!("{:016x}", fnv1a64(b"http://mole.61.com.tw/a.swf"))[0..2].to_string())
                .join(format!("{:016x}.swfcache", fnv1a64(b"http://mole.61.com.tw/a.swf")))
        );
    }
}
