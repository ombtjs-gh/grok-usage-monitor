const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const appWindow = getCurrentWindow();

const LOCALE_KEY = "grok-usage-monitor.locale";

const I18N = {
  en: {
    usageSubtitle: "Usage",
    connected: "connected",
    refresh: "Refresh",
    minimize: "Minimize",
    close: "Hide to tray",
    langSwitch: "Switch to Japanese",
    emptyTitle: "Add an account",
    emptyHint: "Import Grok CLI auth, or sign in with the browser",
    importCli: "CLI import",
    importShort: "Import",
    login: "Log in",
    reset: "Reset",
    lastUpdated: "Updated",
    accounts: "Accounts",
    noAccounts: "No accounts",
    remove: "Remove",
    opacity: "Opacity",
    alwaysOnTop: "Always on top",
    top: "Top",
    every: "Every",
    interval: "Refresh interval",
    statusUpdated: "updated",
    statusRefreshing: "refreshing…",
    statusImporting: "importing…",
    statusLogin: "login…",
    statusLoading: "loading…",
    statusSwitching: "switching…",
    statusRemoving: "removing…",
    statusEvery: (m) => `every ${m}m`,
  },
  ja: {
    usageSubtitle: "使用量",
    connected: "接続済み",
    refresh: "更新",
    minimize: "最小化",
    close: "トレイに隠す",
    langSwitch: "Switch to English",
    emptyTitle: "アカウントを追加",
    emptyHint: "Grok CLI の認証、またはブラウザログイン",
    importCli: "CLI 取り込み",
    importShort: "取込",
    login: "ログイン",
    reset: "リセット",
    lastUpdated: "最終更新",
    accounts: "アカウント",
    noAccounts: "アカウントなし",
    remove: "削除",
    opacity: "透明度",
    alwaysOnTop: "常に前面",
    top: "前面",
    every: "間隔",
    interval: "更新間隔",
    statusUpdated: "更新しました",
    statusRefreshing: "更新中…",
    statusImporting: "取り込み中…",
    statusLogin: "ログイン…",
    statusLoading: "取得中…",
    statusSwitching: "切替中…",
    statusRemoving: "削除中…",
    statusEvery: (m) => `間隔 ${m}分`,
  },
};

/** @type {"en"|"ja"} */
let locale = loadLocale();

const el = {
  empty: document.getElementById("empty-state"),
  usage: document.getElementById("usage-panel"),
  accountLabel: document.getElementById("account-label"),
  usagePct: document.getElementById("usage-pct"),
  usageBarFill: document.getElementById("usage-bar-fill"),
  resetText: document.getElementById("reset-text"),
  fetchedText: document.getElementById("fetched-text"),
  breakdown: document.getElementById("breakdown"),
  accountList: document.getElementById("account-list"),
  status: document.getElementById("status"),
  opacity: document.getElementById("opacity"),
  opacityVal: document.getElementById("opacity-val"),
  alwaysOnTop: document.getElementById("always-on-top"),
  interval: document.getElementById("interval"),
  btnLang: document.getElementById("btn-lang"),
  langCode: document.getElementById("lang-code"),
  btnRefresh: document.getElementById("btn-refresh"),
  btnMinimize: document.getElementById("btn-minimize"),
  btnClose: document.getElementById("btn-close"),
  btnImport: document.getElementById("btn-import"),
  btnLogin: document.getElementById("btn-login"),
  btnImport2: document.getElementById("btn-import-2"),
  btnLogin2: document.getElementById("btn-login-2"),
};

let busy = false;
let snapshot = null;
/** Last status kind for re-translate after locale switch */
let lastStatusKey = null;
let lastStatusKind = "";

function loadLocale() {
  try {
    const v = localStorage.getItem(LOCALE_KEY);
    if (v === "en" || v === "ja") return v;
  } catch {
    /* ignore */
  }
  // Prefer Japanese when browser/OS is ja
  const nav = (navigator.language || "").toLowerCase();
  return nav.startsWith("ja") ? "ja" : "en";
}

function saveLocale(lang) {
  try {
    localStorage.setItem(LOCALE_KEY, lang);
  } catch {
    /* ignore */
  }
}

function t(key) {
  const pack = I18N[locale] || I18N.en;
  const v = pack[key] ?? I18N.en[key] ?? key;
  return typeof v === "function" ? v : v;
}

function setStatus(msg, kind = "", statusKey = null) {
  el.status.textContent = msg || "";
  el.status.className = "status" + (kind ? ` ${kind}` : "");
  lastStatusKey = statusKey;
  lastStatusKind = kind;
}

function setStatusKey(key, kind = "ok") {
  setStatus(t(key), kind, key);
}

function setBusy(v) {
  busy = v;
  [
    el.btnRefresh,
    el.btnImport,
    el.btnLogin,
    el.btnImport2,
    el.btnLogin2,
    el.btnLang,
  ].forEach((b) => {
    if (b) b.disabled = v;
  });
}

function usageTone(pct) {
  if (pct >= 90) return "crit";
  if (pct >= 70) return "hot";
  return "ok";
}

function usageColor(pct) {
  if (pct >= 90) return "#e85d5d";
  if (pct >= 70) return "#d4a017";
  return "#e8e8e8";
}

/** Format as YYYY-MM-DD HH:MM (local time). */
function formatYmdHm(iso) {
  if (!iso) return "----/--/-- --:--";
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return "----/--/-- --:--";
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    const h = String(d.getHours()).padStart(2, "0");
    const min = String(d.getMinutes()).padStart(2, "0");
    return `${y}-${m}-${day} ${h}:${min}`;
  } catch {
    return "----/--/-- --:--";
  }
}

function initials(label) {
  const s = String(label || "?").trim();
  if (!s) return "?";
  const local = s.includes("@") ? s.split("@")[0] : s;
  const parts = local.split(/[.\s_-]+/).filter(Boolean);
  if (parts.length >= 2) {
    return (parts[0][0] + parts[1][0]).toUpperCase();
  }
  return local.slice(0, 2).toUpperCase();
}

function applyI18n() {
  document.documentElement.lang = locale;

  // Button shows the language you will switch TO
  const next = locale === "en" ? "JA" : "EN";
  el.langCode.textContent = next;
  el.btnLang.title = t("langSwitch");
  el.btnLang.setAttribute("aria-label", t("langSwitch"));

  document.querySelectorAll("[data-i18n]").forEach((node) => {
    const key = node.getAttribute("data-i18n");
    if (!key) return;
    const val = t(key);
    if (typeof val === "string") node.textContent = val;
  });

  document.querySelectorAll("[data-i18n-title]").forEach((node) => {
    const key = node.getAttribute("data-i18n-title");
    if (!key) return;
    const val = t(key);
    if (typeof val === "string") {
      node.setAttribute("title", val);
      if (node.hasAttribute("aria-label")) {
        node.setAttribute("aria-label", val);
      }
    }
  });

  document.querySelectorAll("[data-i18n-aria]").forEach((node) => {
    const key = node.getAttribute("data-i18n-aria");
    if (!key) return;
    const val = t(key);
    if (typeof val === "string") node.setAttribute("aria-label", val);
  });

  // Re-apply dynamic UI bits (account list labels, empty state already via data-i18n)
  if (snapshot) {
    applySnapshot(snapshot, null, { silent: true, keepStatus: true });
  }

  if (lastStatusKey && I18N.en[lastStatusKey]) {
    setStatusKey(lastStatusKey, lastStatusKind || "ok");
  }
}

function applySettings(settings) {
  if (!settings) return;
  const opacityPct = Math.round((settings.opacity ?? 0.92) * 100);
  el.opacity.value = String(opacityPct);
  el.opacityVal.textContent = `${opacityPct}%`;
  el.alwaysOnTop.checked = !!settings.alwaysOnTop;
  const mins = String(settings.refreshIntervalMinutes ?? 10);
  if ([...el.interval.options].some((o) => o.value === mins)) {
    el.interval.value = mins;
  }
}

function renderUsage(usage) {
  if (!usage) {
    el.usage.classList.add("hidden");
    return;
  }
  el.usage.classList.remove("hidden");
  const pct = Number(usage.percentage ?? 0);
  const clamped = Math.min(Math.max(pct, 0), 100);
  const tone = usageTone(pct);

  el.usagePct.textContent = pct.toFixed(pct % 1 === 0 ? 0 : 1);
  el.usagePct.style.color = usageColor(pct);

  el.usageBarFill.style.width = `${clamped}%`;
  el.usageBarFill.classList.remove("warn", "danger");
  if (tone === "hot") el.usageBarFill.classList.add("warn");
  if (tone === "crit") el.usageBarFill.classList.add("danger");

  el.resetText.textContent = formatYmdHm(usage.resetAt);
  el.fetchedText.textContent = formatYmdHm(usage.fetchedAt);

  const entries = Object.entries(usage.breakdown || {}).sort(
    (a, b) => b[1] - a[1]
  );
  if (entries.length === 0) {
    el.breakdown.innerHTML = "";
    el.breakdown.style.display = "none";
    return;
  }
  el.breakdown.style.display = "";
  el.breakdown.innerHTML = entries
    .map(
      ([name, value]) =>
        `<span class="break-chip">${escapeHtml(name)} <strong>${Number(
          value
        ).toFixed(0)}%</strong></span>`
    )
    .join("");
}

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function renderAccounts(accounts, selectedId, usageByAccount) {
  el.accountList.innerHTML = "";
  if (!accounts || accounts.length === 0) {
    el.accountList.innerHTML = `<li class="account-item"><span class="email" style="color:var(--text-faint)">${escapeHtml(
      t("noAccounts")
    )}</span></li>`;
    return;
  }
  for (const acc of accounts) {
    const li = document.createElement("li");
    li.className =
      "account-item" + (acc.id === selectedId ? " active" : "");
    const label = acc.email || acc.displayName || acc.id.slice(0, 8);
    const u = usageByAccount?.[acc.id];
    let badgeClass = "usage-badge muted";
    let badgeText = "--";
    if (u) {
      const tone = usageTone(u.percentage);
      badgeClass =
        "usage-badge" +
        (tone === "crit" ? " crit" : tone === "hot" ? " hot" : "");
      badgeText = `${Number(u.percentage).toFixed(0)}%`;
    }
    li.innerHTML = `
      <span class="avatar" aria-hidden="true">${escapeHtml(initials(label))}</span>
      <div class="email" title="${escapeHtml(label)}">${escapeHtml(label)}</div>
      <span class="${badgeClass}">${badgeText}</span>
      <button class="btn danger" data-remove="${escapeHtml(acc.id)}" title="${escapeHtml(
      t("remove")
    )}">×</button>
    `;
    li.addEventListener("click", async (e) => {
      if (e.target.closest("[data-remove]")) return;
      if (acc.id === selectedId) return;
      await run("select_account", { accountId: acc.id }, "statusSwitching");
    });
    li.querySelector("[data-remove]").addEventListener("click", async (e) => {
      e.stopPropagation();
      await run("remove_account", { accountId: acc.id }, "statusRemoving");
    });
    el.accountList.appendChild(li);
  }
}

function applySnapshot(snap, errMsg, { silent, keepStatus } = {}) {
  snapshot = snap;
  const accounts = snap?.accounts || [];
  const settings = snap?.settings || {};
  const selectedId = settings.selectedAccountId || accounts[0]?.id;
  const usageByAccount = snap?.usageByAccount || {};
  applySettings(settings);

  if (accounts.length === 0) {
    el.empty.classList.remove("hidden");
    el.usage.classList.add("hidden");
    el.accountLabel.textContent = t("usageSubtitle");
  } else {
    el.empty.classList.add("hidden");
    const selected =
      accounts.find((a) => a.id === selectedId) || accounts[0];
    el.accountLabel.textContent =
      selected?.email || selected?.displayName || t("connected");
    renderUsage(snap.usage);
  }
  renderAccounts(accounts, selectedId, usageByAccount);

  if (keepStatus) return;

  if (errMsg) setStatus(errMsg, "error", null);
  else if (snap?.error) setStatus(snap.error, "error", null);
  else if (snap?.usage && !silent) setStatusKey("statusUpdated", "ok");
}

/**
 * @param {string} cmd
 * @param {object} args
 * @param {string} pendingKey i18n key for pending status
 */
async function run(cmd, args = {}, pendingKey = "statusLoading") {
  if (busy) return;
  setBusy(true);
  setStatusKey(pendingKey, "");
  try {
    const snap = await invoke(cmd, args);
    applySnapshot(snap);
    if (!snap?.error) {
      setTimeout(() => {
        if (lastStatusKey === "statusUpdated") setStatus("", "", null);
      }, 1200);
    }
    return snap;
  } catch (e) {
    const msg = typeof e === "string" ? e : e?.message || String(e);
    setStatus(msg, "error", null);
    try {
      const snap = await invoke("get_snapshot");
      applySnapshot(snap, msg);
    } catch {
      /* ignore */
    }
  } finally {
    setBusy(false);
  }
}

function toggleLocale() {
  locale = locale === "en" ? "ja" : "en";
  saveLocale(locale);
  applyI18n();
}

async function init() {
  applyI18n();

  el.btnLang.addEventListener("click", (e) => {
    e.stopPropagation();
    toggleLocale();
  });

  el.btnRefresh.addEventListener("click", () =>
    run("refresh_usage", {}, "statusRefreshing")
  );
  el.btnImport.addEventListener("click", () =>
    run("import_grok_cli", {}, "statusImporting")
  );
  el.btnLogin.addEventListener("click", () =>
    run("login_oauth", {}, "statusLogin")
  );
  el.btnImport2.addEventListener("click", () =>
    run("import_grok_cli", {}, "statusImporting")
  );
  el.btnLogin2.addEventListener("click", () =>
    run("login_oauth", {}, "statusLogin")
  );

  el.btnMinimize.addEventListener("click", async () => {
    try {
      await appWindow.minimize();
    } catch (e) {
      setStatus(String(e), "error", null);
    }
  });

  el.btnClose.addEventListener("click", async () => {
    try {
      await appWindow.hide();
    } catch (e) {
      setStatus(String(e), "error", null);
    }
  });

  let opacityTimer = null;
  el.opacity.addEventListener("input", () => {
    const pct = Number(el.opacity.value);
    el.opacityVal.textContent = `${pct}%`;
    clearTimeout(opacityTimer);
    opacityTimer = setTimeout(async () => {
      try {
        await invoke("set_opacity", { opacity: pct / 100 });
      } catch (e) {
        setStatus(String(e), "error", null);
      }
    }, 50);
  });

  el.alwaysOnTop.addEventListener("change", async () => {
    try {
      await invoke("set_always_on_top", {
        enabled: el.alwaysOnTop.checked,
      });
    } catch (e) {
      setStatus(String(e), "error", null);
    }
  });

  el.interval.addEventListener("change", async () => {
    try {
      await invoke("set_refresh_interval", {
        minutes: Number(el.interval.value),
      });
      const msg = I18N[locale].statusEvery(el.interval.value);
      setStatus(msg, "ok", null);
    } catch (e) {
      setStatus(String(e), "error", null);
    }
  });

  try {
    await listen("usage-updated", (event) => {
      applySnapshot(event.payload, null, { silent: true });
    });
  } catch (e) {
    console.warn("event listen failed", e);
  }

  try {
    const snap = await invoke("get_snapshot");
    applySnapshot(snap, null, { silent: true });
    if ((snap.accounts || []).length > 0 && !snap.usage) {
      await run("refresh_usage", {}, "statusLoading");
    }
  } catch (e) {
    setStatus(String(e), "error", null);
  }
}

window.addEventListener("DOMContentLoaded", init);
