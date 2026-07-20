const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

const appWindow = getCurrentWindow();

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

function setStatus(msg, kind = "") {
  el.status.textContent = msg || "";
  el.status.className = "status" + (kind ? ` ${kind}` : "");
}

function setBusy(v) {
  busy = v;
  [
    el.btnRefresh,
    el.btnImport,
    el.btnLogin,
    el.btnImport2,
    el.btnLogin2,
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
    el.accountList.innerHTML =
      '<li class="account-item"><span class="email" style="color:var(--text-faint)">No accounts</span></li>';
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
      <button class="btn danger" data-remove="${escapeHtml(acc.id)}" title="削除">×</button>
    `;
    li.addEventListener("click", async (e) => {
      if (e.target.closest("[data-remove]")) return;
      if (acc.id === selectedId) return;
      await run("select_account", { accountId: acc.id }, "切替中…");
    });
    li.querySelector("[data-remove]").addEventListener("click", async (e) => {
      e.stopPropagation();
      await run("remove_account", { accountId: acc.id }, "削除中…");
    });
    el.accountList.appendChild(li);
  }
}

function applySnapshot(snap, errMsg, { silent } = {}) {
  snapshot = snap;
  const accounts = snap?.accounts || [];
  const settings = snap?.settings || {};
  const selectedId = settings.selectedAccountId || accounts[0]?.id;
  const usageByAccount = snap?.usageByAccount || {};
  applySettings(settings);

  if (accounts.length === 0) {
    el.empty.classList.remove("hidden");
    el.usage.classList.add("hidden");
    el.accountLabel.textContent = "Usage";
  } else {
    el.empty.classList.add("hidden");
    const selected =
      accounts.find((a) => a.id === selectedId) || accounts[0];
    el.accountLabel.textContent =
      selected?.email || selected?.displayName || "connected";
    renderUsage(snap.usage);
  }
  renderAccounts(accounts, selectedId, usageByAccount);

  if (errMsg) setStatus(errMsg, "error");
  else if (snap?.error) setStatus(snap.error, "error");
  else if (snap?.usage && !silent) setStatus("updated", "ok");
}

async function run(cmd, args = {}, pending = "…") {
  if (busy) return;
  setBusy(true);
  setStatus(pending);
  try {
    const snap = await invoke(cmd, args);
    applySnapshot(snap);
    if (!snap?.error) {
      setTimeout(() => {
        if (el.status.textContent === "updated") setStatus("");
      }, 1200);
    }
    return snap;
  } catch (e) {
    const msg = typeof e === "string" ? e : e?.message || String(e);
    setStatus(msg, "error");
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

async function init() {
  el.btnRefresh.addEventListener("click", () =>
    run("refresh_usage", {}, "refreshing…")
  );
  el.btnImport.addEventListener("click", () =>
    run("import_grok_cli", {}, "importing…")
  );
  el.btnLogin.addEventListener("click", () =>
    run("login_oauth", {}, "login…")
  );
  el.btnImport2.addEventListener("click", () =>
    run("import_grok_cli", {}, "importing…")
  );
  el.btnLogin2.addEventListener("click", () =>
    run("login_oauth", {}, "login…")
  );

  el.btnMinimize.addEventListener("click", async () => {
    try {
      await appWindow.minimize();
    } catch (e) {
      setStatus(String(e), "error");
    }
  });

  el.btnClose.addEventListener("click", async () => {
    try {
      await appWindow.hide();
    } catch (e) {
      setStatus(String(e), "error");
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
        setStatus(String(e), "error");
      }
    }, 50);
  });

  el.alwaysOnTop.addEventListener("change", async () => {
    try {
      await invoke("set_always_on_top", {
        enabled: el.alwaysOnTop.checked,
      });
    } catch (e) {
      setStatus(String(e), "error");
    }
  });

  el.interval.addEventListener("change", async () => {
    try {
      await invoke("set_refresh_interval", {
        minutes: Number(el.interval.value),
      });
      setStatus(`every ${el.interval.value}m`, "ok");
    } catch (e) {
      setStatus(String(e), "error");
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
      await run("refresh_usage", {}, "loading…");
    }
  } catch (e) {
    setStatus(String(e), "error");
  }
}

window.addEventListener("DOMContentLoaded", init);
