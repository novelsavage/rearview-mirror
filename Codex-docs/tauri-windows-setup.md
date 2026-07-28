# Rearview Mirror: Windows / Tauri 開発環境

更新日: 2026-07-28

## このPCで確認した環境

| 道具 | 役割 | 確認した状態 |
|---|---|---|
| Node.js | TypeScript画面のビルド | v24.13.1 |
| npm | フロントエンド依存ライブラリの管理 | v11.8.0 |
| Rustup | Rustの導入・更新 | v1.29.0 |
| rustc | Rustコンパイラ | v1.97.1 |
| Cargo | Rust依存管理・ビルド | v1.97.1 |
| Visual Studio Build Tools 2022 | Windows用のリンク処理とSDK | C++ Desktop Tools導入済み |
| Microsoft Edge WebView2 Runtime | Tauriの画面・カメラ権限 | v150.0.4078.99 |

Rustでアプリを書くが、C++を書く必要はない。Build Toolsに含まれるMSVCリンカーが、Rustで生成した部品とWindowsのAPIを結合して `.exe` にする。

## 役割の分担

```text
Rust / Tauri
  ├─ グローバルショートカット
  ├─ タスクトレイ
  ├─ 枠なしウィンドウ
  └─ 長押し中のマウス移動

TypeScript / HTML / CSS
  ├─ カメラ映像
  ├─ 左右反転
  └─ 設定画面

WebView2
  └─ Windows上でTypeScript画面を表示し、カメラ権限を扱う
```

## 開発時の起動

PowerShellで次を実行する。

```powershell
Set-Location C:\Projects\rearview-mirror\app
npm run tauri dev
```

- 初回は設定画面が出る
- 2回目以降はタスクトレイに常駐する
- トレイアイコンを右クリックして `設定` を選ぶ
- `Ctrl + Alt + Space` を押している間、ミラーを表示する
- 初回のカメラ許可は設定画面の `カメラを確認・許可する` を自分で押して与える

## 確認すること

1. 設定画面でカメラ許可を与える
2. `Ctrl + Alt + Space` を押してミラーが出ることを確認する
3. 押したままマウスを動かし、ミラーが追従することを確認する
4. キーを離し、映像が消えてカメラ利用表示も消えることを確認する
5. タスクトレイから `設定` を開き、600 px以外のサイズも試す

## ビルド

```powershell
Set-Location C:\Projects\rearview-mirror\app
npm run tauri build
```

生成物:

- 実行ファイル: `src-tauri\target\release\rearview-mirror.exe`
- インストーラー: `src-tauri\target\release\bundle\nsis\Rearview Mirror_0.1.0_x64-setup.exe`
- MSI: `src-tauri\target\release\bundle\msi\Rearview Mirror_0.1.0_x64_en-US.msi`

## 再導入が必要になった場合

```powershell
winget install --id Rustlang.Rustup --exact
winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id Microsoft.EdgeWebView2Runtime --exact
```

Visual Studio Build Toolsのwingetインストーラーは一時フォルダから起動する。導入後に一時ファイルを参照する警告が出ても、`link.exe` が `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\...` に存在すれば導入は完了している。
