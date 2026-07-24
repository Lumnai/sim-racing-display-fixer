const { invoke } = window.__TAURI__.core;
const $ = (id) => document.getElementById(id);

function toast(msg) {
  const t = $("toast");
  t.textContent = msg;
  t.classList.remove("hidden");
  clearTimeout(toast._t);
  toast._t = setTimeout(() => t.classList.add("hidden"), 4000);
}

function busy(on, text) {
  $("busyText").textContent = text || "Working...";
  $("busy").classList.toggle("hidden", !on);
}

async function refresh() {
  let s;
  try {
    s = await invoke("get_status");
  } catch (e) {
    toast("" + e);
    return;
  }

  $("bigWidth").textContent = s.current_width > 0 ? s.current_width : "-";
  $("summary").textContent = s.summary;
  $("adapters").textContent = s.adapters.length ? "GPUs: " + s.adapters.join(", ") : "";

  const chip = $("chip");
  if (!s.has_profile) {
    chip.textContent = "not set up";
    chip.className = "chip neutral";
  } else if (s.matches) {
    chip.textContent = "CORRECT";
    chip.className = "chip good";
  } else {
    chip.textContent = "NEEDS FIXING";
    chip.className = "chip bad";
  }

  $("onboarding").classList.toggle("hidden", s.has_profile);
  $("dashboard").classList.toggle("hidden", !s.has_profile);
  $("autofix").checked = s.autofix_enabled;
  $("fixBtn").disabled = s.matches;
}

async function capture() {
  busy(true, "Saving your display...");
  try {
    toast(await invoke("capture_profile"));
  } catch (e) {
    toast("" + e);
  }
  busy(false);
  refresh();
}

async function fixNow() {
  busy(true, "Fixing... screens may blink");
  try {
    toast(await invoke("fix_now"));
  } catch (e) {
    toast("" + e);
  }
  busy(false);
  refresh();
}

async function toggleAutofix() {
  const enable = $("autofix").checked;
  busy(true, enable ? "Turning on auto-fix..." : "Turning off auto-fix...");
  try {
    await invoke("set_autofix", { enabled: enable });
    toast(enable ? "Auto-fix is on." : "Auto-fix is off.");
  } catch (e) {
    toast("" + e);
  }
  busy(false);
  refresh();
}

async function checkUpdate(quiet) {
  try {
    const u = await invoke("check_update");
    if (u.available) {
      $("updateText").textContent = "Update available: v" + u.version;
      $("updateBanner").classList.remove("hidden");
    } else if (!quiet) {
      toast("You are on the latest version.");
    }
  } catch (e) {
    if (!quiet) toast("Update check failed: " + e);
  }
}

async function installUpdate() {
  busy(true, "Downloading update...");
  try {
    await invoke("install_update");
  } catch (e) {
    busy(false);
    toast("" + e);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  $("captureBtn").onclick = capture;
  $("recaptureBtn").onclick = capture;
  $("fixBtn").onclick = fixNow;
  $("autofix").onchange = toggleAutofix;
  $("checkUpdateBtn").onclick = () => checkUpdate(false);
  $("updateBtn").onclick = installUpdate;
  refresh();
  checkUpdate(true);
});
