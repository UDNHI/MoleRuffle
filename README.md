# MoleRuffle 台版

基於 [MoleRuffle](https://github.com/moleworld-dev/MoleRuffle) 與 [Ruffle](https://github.com/ruffle-rs/ruffle) 的非官方客戶端，讓 Apple Silicon（M 系列）Mac 連線遊玩台版摩爾莊園。

目前版本：**v0.1.9**

## 使用方式

下載 macOS App 後完整解壓縮，將「MoleRuffle 台版.app」放入「應用程式」，雙擊啟動並登入台版帳號。首次啟動會自動準備必要資源，不需要另外安裝 Flash。

App 採本機簽章，尚未經 Apple 公證。

## 已知問題

- 拜訪別人餐廳後返回自己餐廳，服務生可能在端菜時消失。
- 長時間待在餐廳，記憶體會持續增加，玩久了可能變慢；建議不時重開遊戲。
- 家具等遊戲從不卸載的資源會隨遊玩時間慢慢累積。

## 開發

需要 Rust 與 Java（建置 Ruffle 的 ActionScript 內建類別時使用，macOS 可用 `brew install openjdk`）。

```sh
cargo build --release
```

打包 macOS App 用 `packaging/macos/package.py`。

- 引擎固定在 Ruffle `304a3c9`；其中 `ruffle_core` 使用 `vendor/ruffle` 的修補版，改動說明見 [PATCHES.md](vendor/ruffle/PATCHES.md)。
- 程式架構見 [架構說明](docs/ARCHITECTURE.md)。

## 版本紀錄

請見 [台版版本紀錄](docs/TAIWAN.md)。

## 來源與授權

台版修改由 UDNHI 維護。原專案來自 [moleworld-dev/MoleRuffle](https://github.com/moleworld-dev/MoleRuffle)，引擎使用 [Ruffle](https://github.com/ruffle-rs/ruffle)；程式碼採 MIT OR Apache-2.0，詳見儲存庫內授權文件。

本專案非淘米官方產品，不包含完整遊戲資源；遊戲內容由官方伺服器載入，相關權利歸原權利人所有。
