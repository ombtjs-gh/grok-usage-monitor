# Screenshot captions (mock data)

All emails and percentages are **fictional mock data** for marketing.

| File | Caption (EN) | Caption (JA) |
|------|--------------|--------------|
| `01-usage-main.png` | Weekly pool at a glance — large usage %, product breakdown, reset & last update as `YYYY-MM-DD HH:MM`. | 週間使用量を一画面で。大きな％、製品別チップ、リセット／最終更新は `YYYY-MM-DD HH:MM`。 |
| `02-accounts.png` | Multi-account list with per-account badges (scrollable). Click a row to switch the active card. | 複数アカウントと使用率バッジ。リストはスクロール、クリックで切替。 |
| `03-onboarding.png` | Get started — import from Grok CLI or sign in with OAuth. | 初期画面。CLI 取り込みまたはブラウザ OAuth。 |
| `04-high-usage.png` | Color shifts as you climb the pool — critical red near the weekly ceiling. | 使用率が上がると赤表示。週間上限の手前で気づける。 |
| `05-controls.png` | Opacity, always-on-top, and refresh interval for a permanent desktop monitor. | 透明度・常に前面・更新間隔。常駐モニター用コントロール。 |

## Mock identities used

- `nova@example.com` — 42%
- `orion@example.com` — 78%
- `lyra.dev@example.com` — 94%
- `quasar@example.org` — 19%
- `vega.team@example.com` — 71%

## Regenerating

```powershell
# From docs/screenshots/ — Chrome headless
$chrome = "C:\Program Files\Google\Chrome\Application\chrome.exe"
# ... see project history or re-run capture script
```

HTML sources: `01-*.html` … `05-*.html` + `styles.css` (copy of app styles).
