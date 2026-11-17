# MoleRuffle 台版架構

## 設計原則：一個共享 Rust 核心＋各平台薄殼

```
                ┌──────────────────────────────────────────┐
                │              moleruffle-core              │
                │  台版設定／Player 組裝／網域偽裝            │
                │  MoleNavigatorInterface（socket 放行）     │
                │  資源快取＋重試（cache.rs）                 │
                │  SimSun 相容字型（simsun_compat.rs）        │
                │  記憶體觀測（mem.rs）／日誌清理（logs.rs）   │
                └────────────────┬─────────────────────────┘
                                 │ 呼叫
                  ┌──────────────┴──────────────┐
                  ▼                             ▼
               desktop                         ios
        （macOS，winit＋wgpu）        （UIView＋CAMetalLayer）
```

| 目錄 | 內容 |
|---|---|
| `crates/moleruffle-core` | 各平台共用的核心 |
| `desktop` | 桌面殼。台版目前只發佈 Apple Silicon macOS |
| `ios` | iOS 殼（xcodegen 工程） |
| `packaging/macos` | App 打包、啟動器、銀行小遊戲補丁 |
| `vendor/ruffle` | 修補過的 `ruffle_core`，見下方「引擎版本」 |

所有平台共用同一份 `ruffle_core`（AVM1／AVM2 是純 Rust 直譯器，執行時不產生機器碼，所以 iOS 不需要 JIT），
只替換平台後端：

| 面向 | 共用 | 平台差異 |
|---|---|---|
| AVM／遊戲邏輯 | `ruffle_core` | 無 |
| 繪圖 | `ruffle_render_wgpu` | macOS／iOS 走 Metal |
| Socket | `ExternalNavigatorBackend`（`tokio::net::TcpStream`） | 無；`SocketMode::Allow` |
| HTTP | reqwest＋rustls（自帶 TLS，不走系統網路堆疊） | 所以 iOS ATS 管不到它 |
| 音訊 | `ruffle_core` 解碼 | cpal（macOS）／coreaudio（iOS） |
| 視窗／輸入 | `Player::handle_event` | winit（桌面）／觸控轉滑鼠（iOS） |

## 台版連線的三個關鍵（都在 core 裡處理）

1. **網域偽裝**：`Client.swf` 發現自己不在官網會 `navigateToURL` 把自己導走。
   → `PlayerBuilder::with_spoofed_url(GAME_SWF_URL)`，讓遊戲以為自己在 `http://mole.61.com.tw/Client.swf`。
2. **直接 TCP 連線**：遊戲用 `flash.net.Socket` 連遊戲伺服器。瀏覽器版必須透過 WebSocket 代理，原生版直接連。
   → `ExternalNavigatorBackend`＋`SocketMode::Allow`＋`MoleNavigatorInterface::confirm_socket` 一律放行。
3. **相對路徑資源**：`version/`、`resource/`、`config/`、`dll/` 都用相對路徑載入。
   → `GAME_BASE_URL = http://mole.61.com.tw/`。

## 資源快取（`cache.rs`）

`CachingNavigator` 包在 Ruffle 的 navigator 外面，只改寫 `fetch`：

- 一律以解析後的完整網址判斷與當快取鍵。
- 帶版本號（遊戲版本管理器加的檔案修改時間，如 `?i1hkoxuw`）的靜態資源永久快取；
  沒有版本號的只快取 24 小時；隨機防快取參數與入口 `Client.swf` 不快取。
- 只寫入檔頭像 SWF／PNG／JPEG／GIF／MP3 的回應。
- 所有 GET 失敗會重試兩次；銀行小遊戲（`resource/map/69.swf`）改由本機補丁提供。
- 每個請求記錄 `moleruffle_core::net` 日誌（來源、狀態、耗時、大小）。

## 中文字型（`lib.rs` 的 `MoleUiBackend`、`simsun_compat.rs`）

遊戲九成以上的文字框指定 SimSun／宋体，排版照 SimSun 的行高（1.00 em）設計；macOS 沒有 SimSun。

- SimSun 家族：改用行高相同的 SimSong、STSong；都沒有時用打包的 Noto Sans TC，並在記憶體裡把行高改成 SimSun 的比例。
- Arial、Tahoma、Times New Roman 等：英文用原字型，中文逐字退到宋體。
- 其他名稱：精確比對，失敗再退到 PingFang TC 等一定有中文的字型。

## 安全性

- `MoleNavigatorInterface::open_file` 一律拒絕：遊戲走明文 HTTP，被竄改的內容不能借 `file://` 讀本機檔案。
- 遊戲程序固定使用台灣時區（`TZ=Asia/Taipei`，只影響遊戲程序，不改系統時區）。

## 引擎版本

Ruffle 沒有發佈到 crates.io，主線也常改動 API，所以所有 Ruffle 依賴固定在實測可用的 commit：

```
rev = "304a3c9dcaf42ed6d4b1c8bbd05f10d21f407c2e"
```

其中 `ruffle_core` 透過根目錄 `Cargo.toml` 的 `[patch]` 改用 `vendor/ruffle/core`：同一個 rev，
加上「卸載 SWF 時釋放其符號庫」的修補（Ruffle 原本會讓每個載入過的 SWF 永遠留在記憶體）。
改了哪些地方、升級時怎麼處理，見 [vendor/ruffle/PATCHES.md](../vendor/ruffle/PATCHES.md)。

升級引擎＝改 rev、重新複製並套用 `vendor/ruffle` 的修補、重測全部玩法、提交新的 `Cargo.lock`。

## 觀測與日誌

- 啟動器把每次執行的輸出寫到資料夾（`~/Library/Application Support/MoleRuffle-Taiwan-Test`）裡的日誌，
  啟動時每種只保留最新 20 份。
- 桌面版每 30 秒記錄 `Memory footprint`：記憶體用量、仍存在的 SWF 數量與已釋放的數量、重複最多的 SWF。
- 單一階段耗時過長時記錄 `Slow UI work`。

## 已知問題與待辦

- **餐廳記憶體**：遊戲沒有主動卸載的 SWF 仍不會被釋放，長時間待在餐廳記憶體會持續增加。
  進一步的做法（偵測不再被使用的 SWF 並自動釋放）有讓畫面元素消失的風險，暫緩。
- **服務生消失**：拜訪別人餐廳後回到自己餐廳，服務生可能在端菜時消失；推測是同一個 Loader 重複換裝的載入競爭，未證實。
- **輸入**：觸控板捲動以像素除以 100 換算成行數，會一格一格跳；注音輸入法的選字框位置未對齊文字框。
- **外部連結與全螢幕**：`navigate_to_website` 只記日誌，`set_fullscreen` 沒有實作。
- **銀行補丁**：打包版準備補丁失敗時整個遊戲無法啟動，應改為退回官方原檔照常啟動。
- **全玩法相容**：Flex `spark`／TLF 富文字元件等是 Ruffle 已知的弱項，需逐一驗證各小遊戲。
