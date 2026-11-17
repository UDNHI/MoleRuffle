# ruffle_core 本地修補版

這裡是 Ruffle `304a3c9dcaf42ed6d4b1c8bbd05f10d21f407c2e`（2026-06-24）的 `core/` 目錄，
專案根目錄的 `Cargo.toml` 用 `[patch."https://github.com/ruffle-rs/ruffle"]` 讓所有依賴改用這一份。
其他 Ruffle crate（render、swf、frontend-utils…）仍從官方倉庫同一個 rev 取得。

升級 Ruffle 時：把新版的 `core/` 重新複製進來，再照下面兩段重新套用；
若官方已合併 SWF 釋放的修正，就刪掉 `vendor/ruffle` 與根目錄的 `[patch]`。

## 1. 建置設定（讓 core 能脫離 Ruffle 倉庫單獨編譯）

- `vendor/ruffle/Cargo.toml`：只含 core 相關成員的 workspace，`[workspace.package]`、
  `[workspace.dependencies]`、`[workspace.lints]` 原樣複製自 Ruffle 根目錄。
- `core/Cargo.toml`、`core/common/Cargo.toml`、`core/build_playerglobal/Cargo.toml`：
  指向 core 以外的 `path = "../…"` 依賴（render、naga-agal、video、wstr、swf、flv、tools/asc）
  改成同一 rev 的 git 依賴。

## 2. 卸載時釋放 SWF（記憶體外洩修正）

**問題**：`Library` 以弱引用登記每部載入過的影片，但登記的值 `MovieLibrary` 自己強引用影片，
其中的角色與 AVM2 類別也都強引用影片，所以影片永遠不會被釋放，連同它的位圖、形狀（含 GPU 資源）。
餐廳裡客人來來去去，5 分鐘就累積近 400 個 SWF、約 7 GB。官方回報：ruffle-rs/ruffle#24864。

**做法**：移植 ruffle-rs/ruffle#24918（尚未合併），並做五點調整：

1. 除了 `Loader.unloadAndStop()`，一般的 `Loader.unload()` 也會釋放（遊戲兩者都用）。
   只有 `unloadAndStop(true)` 會在本次更新後強制完整垃圾回收。
2. 影片的 AVM2 網域與其他影片共用時（例如載入到 `ApplicationDomain.currentDomain` 的素材庫）
   不釋放，避免「卸載後仍用它的類別建立物件」的寫法失效。
3. 影片用 `SymbolClass` 把符號連結到類別時也不釋放。新天使園戰鬥會載入怪物、塔、技能 SWF，
   先用 `getDefinition` 取出類別、立刻卸載，之後才建立物件；清空的話會出現
   `Tried to instantiate non-registered character ID`，戰鬥場景畫不出來。
4. **卸載時不立刻清空**：先記下當時的內容（根剪輯和底下的剪輯、位圖）的弱引用，
   每次更新檢查一次，全部被垃圾回收後才清空（日誌 `Released after content was collected`）。
   新天使園戰鬥地圖就是反例：地圖 SWF 載入後內容先被存起來、Loader 立刻卸載，
   之後才掛到戰鬥圖層，子剪輯此時才從符號庫建立。立刻清空的話整張地圖畫不出來。
5. **不刪除符號庫登記，只清空內容**（角色、匯出符號、字型、JPEG 表），保留影片與 AVM2 網域。
   上游 PR 是整筆刪除，但這個 Ruffle 版本有很多地方假設符號庫一定存在
   （`library_for_movie(..).unwrap()`，或 `library_for_movie_mut` 查不到就建空的、之後取網域時崩潰），
   而卸載後的舊剪輯仍可能跑幀腳本。整筆刪除的版本實測在餐廳約 1 分鐘就在
   `movie_clip.rs` 的 `run_frame_scripts` 崩潰。代價是每次卸載留下影片原始資料與類別定義。

**改動的檔案**：

- `src/library.rs`：`Library::release_movie`（回傳 `ReleaseOutcome`）、`Library::is_movie_released`、
  `MovieLibrary::strip` 與 `released` 旗標、`Avm2ClassRegistry::release_movie`、`MovieLibraries::get_mut`。
- `src/avm2/globals/flash/display/loader.rs`：`unload` 改走 `unload_and_release`；
  新增原生 `unload_and_stop`。
- `src/avm2/globals/flash/display/Loader.as`：`unloadAndStop` 改為 `native`。
- `src/avm2.rs`：`Avm2::unregister_broadcast_listeners_of_movie`。
- `src/context.rs`、`src/player.rs`：`needs_gc` 旗標，在 `Player::update` 收尾時 `finish_cycle`。

每次卸載會記一行 `ruffle_core::unload` 的 info 日誌（`Pending` / `SharedDomain` / `HasSymbolClasses` / `Untracked` / `NotFound`）。

**已知限制**（與上游 PR 相同）：卸載後仍存活、由該影片建立的物件保有已拿到的東西，
但無法再從它建立新的符號實例；`unloadAndStop` 停止聲音、串流、計時器與舞台監聽仍未實作。

## 倒帶時時間軸物件疊到腳本子物件上面

**問題**：剪輯倒帶（例如 `gotoAndStop(1)` 從第 2 格跳回）時，Ruffle 先拿掉第 2 格的時間軸物件，
再在同一深度放上第 1 格的物件。如果上面沒有其他時間軸物件，新物件會被加到顯示清單最上層，
蓋住用 `addChild` 加入的子物件。陽光牧場向尤尤賣農產品時，格子剪輯在滑鼠移入、移出時於第 1、2 格切換，
移出後格子背景蓋住農產品圖示與數量，看起來像圖示消失。上游 master 同樣如此。

**做法**（`display_object/movie_clip.rs` 的 `run_goto`、`display_object/container.rs` 的
`restore_rewound_position`）：倒帶時記住被拿掉的時間軸物件原本的位置；同一深度重新放置後，
若它和原位置之間全是腳本加入的子物件，就把它移回原位。其他情況不動。
