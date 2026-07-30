# Rearview Mirror

PC の Web カメラを使い、後方を短時間だけ確認するための Windows アプリです。

講義中などに振り向かず、`Ctrl + Alt + Space` を押している間だけ小さなミラー映像を表示します。映像の録画・保存・送信や、マイクの使用は行いません。

## 主な機能

- 押している間だけ表示するグローバルショートカット
- タイトルバーやボタンのない、カメラ映像だけのミラーウィンドウ
- タスクバー高に揃えた、横へ引き延ばす4:1ミラー表示
- 白黒表示の切り替え
- 初期状態は、左右反転オン・白黒オフ・マウス移動オン・ショートカット切替表示オン
- `Ctrl + Alt + Space` の長押し表示／切替表示を設定から選択
- ショートカットを押したままマウスを動かす位置調整
- タスクトレイから開く設定画面
- Windows ログオン時の自動起動
- 非表示時にカメラストリームを停止

## 動作環境

- Windows 10 / 11
- WebView2 Runtime
- カメラを使用できる PC

## 開発を始める

必要な環境とセットアップ手順は [環境構築ガイド](Codex-docs/tauri-windows-setup.md) を参照してください。

```powershell
Set-Location C:\Projects\rearview-mirror\app
npm install
npm run tauri dev
```

## 配布用ビルド

```powershell
Set-Location C:\Projects\rearview-mirror\app
npm run tauri build
```

Windows 向けの NSIS インストーラーは `src-tauri\target\release\bundle\nsis` に生成されます。生成物は Git で管理せず、GitHub Releases で配布します。

## プライバシー

このアプリはライブプレビューのみを目的としています。録画、静止画保存、音声取得、クラウド送信、人物認識は実装していません。利用する場所のルールと周囲のプライバシーに配慮して使ってください。

## ライセンス

ライセンスは未定です。利用・再配布の条件を決めるまでは、ソースコードの利用許諾は行いません。
