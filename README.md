# Grok Usage Monitor

xAI Grok（SuperGrok など）の**週間使用量プール**を、デスクトップ上に軽量表示するモニターアプリです。

- 複数アカウント対応（OAuth / Grok CLI 取り込み）
- 使用率バー・製品別内訳・リセット日時
- Always on Top / ウィンドウ透明度 / 高さリサイズ
- システムトレイ常駐・自動リフレッシュ
- **OS キーチェーン**にトークンを保存

設計の詳細は [docs/DESIGN.md](docs/DESIGN.md) を参照してください。

## 必要環境

- [Rust](https://rustup.rs/) (1.77+)
- Windows: WebView2（通常は OS に同梱）
- macOS / Linux: 対応する WebKitGTK 等（Tauri 2 要件）
- Linux: Secret Service（例: GNOME Keyring / KWallet）があるとキーチェーン保存が有効

開発時は [Tauri CLI](https://v2.tauri.app/start/prerequisites/) があると便利です。

```bash
cargo install tauri-cli --version "^2" --locked
```

## 起動（開発）

```bash
cd grok-usage-monitor
cargo tauri dev
```

または:

```bash
cd src-tauri
cargo run
```

## ビルド

```bash
cargo tauri build
```

成果物は `src-tauri/target/release/`（およびインストーラ）に出力されます。

## 使い方

1. アプリを起動
2. **Grok CLI から取り込み**（`~/.grok/auth.json`）または **ブラウザでログイン**（OAuth PKCE）
3. 週間使用率がカード上に表示されます
4. 透明度・常に前面・更新間隔は下部コントロールから変更

### データ保存先

| 内容 | 保存先 |
|------|--------|
| アカウント・トークン | **OS キーチェーン**（service: `com.grok.usage-monitor`） |
|  | Windows Credential Manager / macOS Keychain / Linux Secret Service |
| UI 設定 | `~/.grok-monitor/settings.json` |

起動時に旧形式の `~/.grok-monitor/accounts.json` があればキーチェーンへ自動移行し、平文ファイルは削除します。キーチェーンが使えない環境ではファイル保存にフォールバックします。

## GitHub Actions リリース

[`.github/workflows/release.yml`](.github/workflows/release.yml) が次のタイミングでインストーラをビルドし、Release に添付します。

| トリガー | 動作 |
|----------|------|
| **GitHub Release を Publish** | その Release に Windows / macOS / Linux 成果物をアップロード |
| **Actions → Release → Run workflow** | アプリバージョンの draft Release を作成して成果物を添付 |

### 手順（推奨）

1. リポジトリ設定 → **Actions** → **Workflow permissions** で **Read and write permissions** を有効化
2. GitHub 上で **Draft a new release** → タグ例 `v0.1.0` → **Publish release**
3. Actions の `Release` ワークフローが走り、各 OS の `.msi` / `.exe` / `.dmg` / `.deb` 等が Release に付く

手動:

```text
Actions → Release → Run workflow
```

macOS のコード署名・公証は未設定です。必要なら [Tauri の署名ガイド](https://v2.tauri.app/distribute/sign/macos/) に沿って Secrets を追加してください。

## 使用量 API について

公式の週間使用量公開 API は無いため、Grok Build CLI と同様の内部エンドポイントを利用しています。

- `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`
- 認証: OAuth access token（`auth.x.ai`）+ `X-XAI-Token-Auth: xai-grok-cli`

エンドポイント変更時は取得失敗を UI に表示します。個人の読み取り専用利用を想定しています。

## 実装フェーズ

| Phase | 内容 | 状態 |
|-------|------|------|
| 1 MVP | OAuth / CLI 取り込み、使用率、Always-on-Top、透明度 | 実装済 |
| 2 | トレイ常駐、複数アカウント、自動更新イベント | 実装済 |
| 3 | テーマ、設定 UI 充実 | 未 |
| 4 | 通知、**鍵束保存**、CI リリース | 鍵束・CI 実装済 |

### トレイ操作

- **左クリック**: ウィンドウを再表示
- **右クリック**: 表示 / 使用量を更新 / 終了
- **ウィンドウを閉じる**: 終了せずトレイに常駐

## ライセンス

MIT（[LICENSE](LICENSE)）
