(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);

  // --- Relative times ------------------------------------------------------
  const rtf = window.Intl && Intl.RelativeTimeFormat
    ? new Intl.RelativeTimeFormat(undefined, { numeric: "auto" })
    : null;

  function relative(date) {
    if (!rtf) return date.toLocaleString();
    const secs = Math.round((date.getTime() - Date.now()) / 1000);
    const abs = Math.abs(secs);
    if (abs < 45) return rtf.format(0, "second");
    if (abs < 3600) return rtf.format(Math.round(secs / 60), "minute");
    if (abs < 86400) return rtf.format(Math.round(secs / 3600), "hour");
    if (abs < 86400 * 30) return rtf.format(Math.round(secs / 86400), "day");
    return date.toLocaleDateString();
  }

  function renderTimes(root) {
    root.querySelectorAll("time[data-rel]").forEach((el) => {
      const date = new Date(el.getAttribute("datetime"));
      if (Number.isNaN(date.getTime())) return;
      el.textContent = relative(date);
      el.title = date.toLocaleString();
    });
  }

  renderTimes(document);
  setInterval(() => renderTimes(document), 30000);

  // --- Requests ------------------------------------------------------------
  async function api(method, path) {
    const response = await fetch(path, {
      method,
      headers: { Accept: "application/json" },
      cache: "no-store",
      credentials: "same-origin",
    });
    let body = null;
    try {
      body = await response.json();
    } catch (_) {
      body = null;
    }
    if (!response.ok) {
      throw new Error((body && body.message) || "HTTP " + response.status);
    }
    return body;
  }

  // --- Paired devices ------------------------------------------------------
  const list = $("device-list");
  const none = $("no-devices");
  let known = new Set();

  function timeElement(iso) {
    const el = document.createElement("time");
    el.setAttribute("datetime", iso);
    el.dataset.rel = "";
    el.textContent = iso;
    return el;
  }

  function deviceItem(device) {
    const li = document.createElement("li");
    li.className = "device";
    const main = document.createElement("div");
    main.className = "device-main";
    const name = document.createElement("div");
    name.className = "device-name";
    name.textContent = device.name;
    const meta = document.createElement("div");
    meta.className = "muted small";
    const ip = document.createElement("span");
    ip.className = "mono";
    ip.textContent = device.last_ip;
    meta.append(
      "Paired ", timeElement(device.paired_at),
      " · last seen ", timeElement(device.last_seen),
      " · ", ip,
    );
    main.append(name, meta);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn btn-danger btn-small";
    button.textContent = "Revoke";
    button.dataset.revoke = device.id;
    button.dataset.name = device.name;
    li.append(main, button);
    return li;
  }

  async function refreshDevices() {
    const devices = await api("GET", "/devices");
    if (list) {
      list.replaceChildren(...devices.map(deviceItem));
      renderTimes(list);
    }
    if (none) none.hidden = devices.length > 0;
    return devices;
  }

  if (list) {
    known = new Set(
      Array.from(list.querySelectorAll("[data-revoke]")).map((b) => b.dataset.revoke),
    );
    list.addEventListener("click", async (event) => {
      const button = event.target.closest("[data-revoke]");
      if (!button) return;
      if (!window.confirm("Revoke " + button.dataset.name + "? It will have to pair again.")) {
        return;
      }
      button.disabled = true;
      try {
        await api("POST", "/devices/" + encodeURIComponent(button.dataset.revoke) + "/revoke");
        known = new Set((await refreshDevices()).map((d) => d.id));
      } catch (error) {
        button.disabled = false;
        window.alert("Could not revoke: " + error.message);
      }
    });
  }

  // --- Pairing codes -------------------------------------------------------
  const generate = $("generate");
  if (!generate) return;

  const offer = $("offer");
  const code = $("code");
  const expiry = $("expiry");
  const qr = $("qr");
  const open = $("open");
  const hosts = $("hosts");
  const failure = $("generate-error");
  const paired = $("paired");
  let countdown = null;
  let poll = null;

  function stop() {
    clearInterval(countdown);
    clearInterval(poll);
    countdown = null;
    poll = null;
  }

  function groupHex(hex) {
    return (hex.match(/.{1,4}/g) || []).join(" ");
  }

  function tick(expiresAt) {
    const left = Math.max(0, Math.round((expiresAt - Date.now()) / 1000));
    if (left === 0) {
      stop();
      offer.classList.add("expired");
      expiry.classList.add("soon");
      expiry.textContent = "Expired — generate a new code";
      return;
    }
    const minutes = Math.floor(left / 60);
    const seconds = String(left % 60).padStart(2, "0");
    expiry.textContent = "Expires in " + minutes + ":" + seconds;
    expiry.classList.toggle("soon", left <= 30);
  }

  async function watchForPairing() {
    if (document.hidden) return;
    try {
      const devices = await refreshDevices();
      const fresh = devices.find((d) => !known.has(d.id));
      known = new Set(devices.map((d) => d.id));
      if (fresh) {
        stop();
        offer.hidden = true;
        paired.textContent = "Paired " + fresh.name + ".";
        paired.hidden = false;
        generate.textContent = "Generate another code";
      }
    } catch (_) {
      // A missed poll is retried on the next tick.
    }
  }

  generate.addEventListener("click", async () => {
    generate.disabled = true;
    failure.hidden = true;
    try {
      const fresh = await api("POST", "/pairing-codes");
      stop();
      code.textContent = fresh.code;
      // Server-rendered SVG of this server's own pairing link.
      qr.innerHTML = fresh.qr_svg;
      open.href = fresh.uri;
      hosts.replaceChildren(...fresh.hosts.map((host) => {
        const li = document.createElement("li");
        li.className = "mono small";
        li.textContent = host;
        return li;
      }));
      $("fp-short").textContent = fresh.fingerprint_short;
      $("fp-hex").textContent = groupHex(fresh.fingerprint_hex);
      paired.hidden = true;
      offer.classList.remove("expired");
      offer.hidden = false;
      generate.textContent = "Generate a new code";
      const expiresAt = Date.parse(fresh.expires_at);
      tick(expiresAt);
      countdown = setInterval(() => tick(expiresAt), 1000);
      poll = setInterval(watchForPairing, 3000);
    } catch (error) {
      failure.textContent = error.message;
      failure.hidden = false;
    } finally {
      generate.disabled = false;
    }
  });
})();
