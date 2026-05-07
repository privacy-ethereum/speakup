(function () {
  const PROFILES = {
    sha256: {
      url: "_static/profiles/sha256.json",
      description: "Computes a SHA-256 hash of a 2KB message.",
    },
    json: {
      url: "_static/profiles/json.json",
      description: "Parses a 4KB JSON document using serde_json.",
    },
  };

  const selector = document.getElementById("profile-select");
  selector.addEventListener("change", function () {
    const key = this.value;
    if (!key) return;
    const profile = PROFILES[key];
    document.getElementById("profile-description").textContent =
      profile.description;
    fetch(profile.url)
      .then((r) => {
        if (!r.ok) throw new Error("Failed to load profile");
        return r.json();
      })
      .then((data) => {
        DATA = data;
        render(data);
      })
      .catch((e) => {
        alert("Failed to load profile: " + e.message);
      });
  });

  // Cost table help tooltip
  (function () {
    const icon = document.getElementById("cost-help");
    const tip = document.getElementById("cost-tooltip");
    icon.addEventListener("mouseenter", (e) => {
      const r = icon.getBoundingClientRect();
      tip.style.display = "block";
      tip.style.left = r.left - tip.offsetWidth + r.width + "px";
      tip.style.top = r.top - tip.offsetHeight - 8 + "px";
    });
    icon.addEventListener("mouseleave", () => {
      tip.style.display = "none";
    });
  })();

  let DATA = null;

  function render(data) {
    document.getElementById("content").classList.add("visible");
    document.getElementById("bench-name").textContent = data.name;
    resetCosts();
    buildCostEditor(data.stats);
    renderSummary(data.stats);
    updateControlVisibility();
    rebuildHistogram();
  }

  // =================================================================
  // Stats grid
  // =================================================================

  function renderSummary(s) {
    const fmt = (n) => Math.ceil(n).toLocaleString();

    const blocks = DATA.blocks || [];
    const totalInstrs = blocks.reduce(
      (sum, b) => sum + b.instruction_count * b.exec_count,
      0,
    );
    const totalBlocks = blocks.length;
    const memOps = s.memory_loads + s.memory_stores;

    const fmtBytes = (b) => {
      if (b >= 1e9) return (b / 1e9).toFixed(1) + " GB";
      if (b >= 1e6) return (b / 1e6).toFixed(1) + " MB";
      if (b >= 1e3) return (b / 1e3).toFixed(1) + " KB";
      return Math.ceil(b) + " B";
    };

    let totalPublic = 0,
      totalPrivate = 0;
    for (const [name, count] of Object.entries(s.public_cf_histogram || {}))
      totalPublic += getMuls(name, false) * count;
    for (const [name, count] of Object.entries(s.private_cf_histogram || {}))
      totalPrivate += getMuls(name, true) * count;
    const totalMuls = totalPublic + totalPrivate;
    const totalBytesVtoP = (totalMuls * 0.44) / 8;
    const totalBytesPtoV = (totalMuls * 1) / 8;

    const pubPct =
      totalMuls > 0 ? ((totalPublic / totalMuls) * 100).toFixed(1) : "0.0";
    const privPct =
      totalMuls > 0 ? ((totalPrivate / totalMuls) * 100).toFixed(1) : "0.0";

    const fmtTime = (bytes, mbps) => {
      const bits = bytes * 8;
      const seconds = bits / (mbps * 1e6);
      if (seconds < 0.001) return `${(seconds * 1e6).toFixed(0)} \u00b5s`;
      if (seconds < 1) return `${(seconds * 1e3).toFixed(1)} ms`;
      return `${seconds.toFixed(1)} s`;
    };

    let totalPrivateSteps = 0;
    for (const [name, count] of Object.entries(s.private_cf_histogram || {})) {
      const entry = costTable[name];
      totalPrivateSteps += (entry ? entry.steps : 1) * count;
    }

    const privHelper =
      totalPrivateSteps === 0
        ? "All control flow is public \u2014 no step-by-step proving required"
        : "Branches on secret values require step-by-step proving";

    document.getElementById("panel-execution").innerHTML = `
            <div class="summary-panel-title">Execution</div>
            <div style="display:flex;align-items:baseline;gap:8px;margin-bottom:2px;">
                <span style="font-size:1.5rem;font-weight:700;color:#1976d2;">${fmt(totalInstrs)}</span>
                <span style="font-size:0.82rem;color:#757575;">instructions</span>
            </div>
            <div class="summary-secondary">${totalBlocks} blocks \u00b7 ${fmt(memOps)} memory ops \u00b7 ${fmt(s.call_count)} calls</div>
            ${
              totalPrivateSteps > 0
                ? `
                <div style="margin-top:12px;display:flex;align-items:baseline;gap:8px;">
                    <span style="font-size:1.3rem;font-weight:700;color:#f57c00;">${fmt(totalPrivateSteps)}</span>
                    <span style="font-size:0.82rem;color:#757575;">private CPU steps</span>
                </div>
            `
                : ""
            }
            <div class="summary-helper">${privHelper}</div>
        `;

    const pubBarW = totalMuls > 0 ? (totalPublic / totalMuls) * 100 : 100;
    const privBarW = 100 - pubBarW;

    document.getElementById("panel-cost").innerHTML = `
            <div class="summary-panel-title">Proving Cost</div>
            <div style="display:flex;align-items:baseline;gap:8px;margin-bottom:2px;">
                <span style="font-size:1.5rem;font-weight:700;color:#1976d2;">${fmt(totalMuls)}</span>
                <span style="font-size:0.82rem;color:#757575;">sVOLE</span>
            </div>
            <div class="cost-bar">
                <div class="cost-bar-pub" style="width:${pubBarW}%"></div>
                <div class="cost-bar-priv" style="width:${privBarW}%"></div>
            </div>
            <div style="font-size:0.72rem;color:#9e9e9e;margin-bottom:8px;">${pubPct}% public \u00b7 ${privPct}% private</div>
            <div class="summary-secondary">
                ${fmtBytes(totalBytesVtoP)} V\u2192P \u00b7 ${fmtBytes(totalBytesPtoV)} P\u2192V
            </div>
            <div style="margin-top:8px;font-size:0.78rem;color:#757575;">
                Transfer: <strong>${fmtTime(totalBytesVtoP + totalBytesPtoV, 10)}</strong> at 10 Mbps
                <span style="color:#9e9e9e;margin-left:8px;">${fmtTime(totalBytesVtoP + totalBytesPtoV, 100)} at 100 Mbps \u00b7 ${fmtTime(totalBytesVtoP + totalBytesPtoV, 1000)} at 1 Gbps</span>
            </div>
            <div class="summary-helper" style="margin-top:10px;">Cost depends on instruction mix and control flow. Edit weights in the table \u2192</div>
        `;
  }

  // =================================================================
  // Cost table
  // =================================================================

  let mulsPerStep = 700;

  const DEFAULT_COSTS = {
    I32Add: { pub: 32, steps: 1 },
    I32Sub: { pub: 32, steps: 1 },
    I32Mul: { pub: 1500, steps: 32 },
    I32DivU: { pub: 1600, steps: 13 },
    I32RemU: { pub: 1600, steps: 13 },
    I32DivS: { pub: 1700, steps: 17 },
    I32RemS: { pub: 1700, steps: 17 },
    I32And: { pub: 32, steps: 1 },
    I32Or: { pub: 32, steps: 1 },
    I32Xor: { pub: 0, steps: 1 },
    I32Eqz: { pub: 31, steps: 1 },
    I32Eq: { pub: 31, steps: 1 },
    I32Ne: { pub: 31, steps: 1 },
    I32LtU: { pub: 32, steps: 1 },
    I32GeU: { pub: 32, steps: 1 },
    I32GtU: { pub: 32, steps: 1 },
    I32LeU: { pub: 32, steps: 1 },
    I32LtS: { pub: 32, steps: 1 },
    I32GeS: { pub: 32, steps: 1 },
    I32GtS: { pub: 32, steps: 1 },
    I32LeS: { pub: 32, steps: 1 },
    I32Shl: { pub: 0, steps: 1 },
    I32ShrU: { pub: 0, steps: 1 },
    I32ShrS: { pub: 0, steps: 1 },
    I32Rotl: { pub: 0, steps: 1 },
    I32Rotr: { pub: 0, steps: 1 },
    I32Clz: { pub: 50, steps: 1 },
    I32Ctz: { pub: 50, steps: 1 },
    I32Popcnt: { pub: 83, steps: 1 },
    Select: { pub: 32, steps: 2 },
    I64Add: { pub: 64, steps: 2 },
    I64Sub: { pub: 64, steps: 2 },
    I64Mul: { pub: 6000, steps: 64 },
    I64And: { pub: 64, steps: 2 },
    I64Or: { pub: 64, steps: 2 },
    I64Xor: { pub: 0, steps: 2 },
    I64Eqz: { pub: 63, steps: 2 },
    I64Eq: { pub: 63, steps: 2 },
    I64Ne: { pub: 63, steps: 2 },
    I64LtU: { pub: 64, steps: 2 },
    I64GeU: { pub: 64, steps: 2 },
    I64GtU: { pub: 64, steps: 2 },
    I64LeU: { pub: 64, steps: 2 },
    I64LtS: { pub: 64, steps: 2 },
    I64GeS: { pub: 64, steps: 2 },
    I64GtS: { pub: 64, steps: 2 },
    I64LeS: { pub: 64, steps: 2 },
    I64Shl: { pub: 0, steps: 1 },
    I64ShrU: { pub: 0, steps: 1 },
    I64ShrS: { pub: 0, steps: 1 },
    I64Rotl: { pub: 0, steps: 1 },
    I64Rotr: { pub: 0, steps: 1 },
    I64Clz: { pub: 100, steps: 2 },
    I64Ctz: { pub: 100, steps: 2 },
    I64Popcnt: { pub: 170, steps: 2 },
    I32Load: { pub: 199, steps: 1 },
    I32Store: { pub: 199, steps: 1 },
    I32Load8S: { pub: 199, steps: 1 },
    I32Load8U: { pub: 199, steps: 1 },
    I32Load16S: { pub: 199, steps: 1 },
    I32Load16U: { pub: 199, steps: 1 },
    I32Store8: { pub: 199, steps: 1 },
    I32Store16: { pub: 199, steps: 1 },
    I64Load: { pub: 398, steps: 2 },
    I64Store: { pub: 398, steps: 2 },
    I64Load8S: { pub: 199, steps: 2 },
    I64Load8U: { pub: 199, steps: 2 },
    I64Load16S: { pub: 199, steps: 2 },
    I64Load16U: { pub: 199, steps: 2 },
    I64Load32S: { pub: 199, steps: 2 },
    I64Load32U: { pub: 199, steps: 2 },
    I64Store8: { pub: 199, steps: 2 },
    I64Store16: { pub: 199, steps: 2 },
    I64Store32: { pub: 199, steps: 2 },
    F32Load: { pub: 199, steps: 1 },
    F32Store: { pub: 199, steps: 1 },
    F64Load: { pub: 398, steps: 2 },
    F64Store: { pub: 398, steps: 2 },
    Copy: { pub: 0, steps: 1 },
    GlobalGet: { pub: 0, steps: 1 },
    GlobalSet: { pub: 0, steps: 1 },
    I32WrapI64: { pub: 0, steps: 1 },
    I64ExtendI32U: { pub: 0, steps: 2 },
    I64ExtendI32S: { pub: 0, steps: 2 },
    I32Extend8S: { pub: 0, steps: 1 },
    I32Extend16S: { pub: 0, steps: 1 },
  };

  let costTable = {};
  function resetCosts() {
    costTable = {};
    for (const [k, v] of Object.entries(DEFAULT_COSTS))
      costTable[k] = { pub: v.pub, steps: v.steps };
  }
  resetCosts();

  function getMuls(opName, isPrivate) {
    const entry = costTable[opName];
    if (!entry) return isPrivate ? mulsPerStep : 32;
    return isPrivate ? entry.steps * mulsPerStep : entry.pub;
  }

  function buildCostEditor(stats) {
    const allOps = new Set([
      ...Object.keys(stats.public_cf_histogram || {}),
      ...Object.keys(stats.private_cf_histogram || {}),
    ]);
    let editor = `<div style="margin-bottom:8px;display:flex;align-items:center;gap:8px;"><label style="font-weight:600;">sVOLE/step: </label><input type="text" inputmode="numeric" pattern="[0-9]*" id="muls-per-step" value="${mulsPerStep}" style="width:60px;text-align:right;padding:1px 3px;border:1px solid #e0e0e0;border-radius:2px;font-size:0.78rem;"><span style="color:#757575;font-size:0.78rem;">= <span id="bytes-per-step">${Math.ceil((mulsPerStep * 1.44) / 8)}</span> B/step</span></div>`;
    editor +=
      '<table style="border-collapse:collapse;font-size:0.78rem;"><tr><th style="padding:2px 6px;">Op</th><th style="padding:2px 6px;">Public<div style="font-weight:400;color:#9e9e9e;font-size:0.68rem;">sVOLE</div></th><th style="padding:2px 6px;">Private<div style="font-weight:400;color:#9e9e9e;font-size:0.68rem;">steps</div></th></tr>';
    for (const op of [...allOps].sort()) {
      const c = costTable[op] || { pub: 0, steps: 0 };
      editor += `<tr>
                <td style="padding:2px 6px;font-weight:500;">${op}</td>
                <td><input type="text" inputmode="numeric" pattern="[0-9]*" value="${c.pub}" data-op="${op}" data-field="pub" style="width:55px;text-align:right;padding:1px 3px;border:1px solid #e0e0e0;border-radius:2px;font-size:0.78rem;"></td>
                <td><input type="text" inputmode="numeric" pattern="[0-9]*" value="${c.steps}" data-op="${op}" data-field="steps" style="width:55px;text-align:right;padding:1px 3px;border:1px solid #e0e0e0;border-radius:2px;font-size:0.78rem;"></td>
            </tr>`;
    }
    editor += "</table>";
    document.getElementById("cost-editor").innerHTML = editor;

    document
      .getElementById("muls-per-step")
      .addEventListener("input", function () {
        mulsPerStep = parseInt(this.value) || 280;
        document.getElementById("bytes-per-step").textContent = Math.ceil(
          (mulsPerStep * 1.44) / 8,
        );
        onCostChange();
      });
    document.querySelectorAll("#cost-editor table input").forEach((inp) => {
      inp.addEventListener("input", () => {
        const op = inp.dataset.op;
        const field = inp.dataset.field;
        if (!costTable[op]) costTable[op] = { pub: 0, steps: 0 };
        costTable[op][field] = parseInt(inp.value) || 0;
        onCostChange();
      });
    });
  }

  function onCostChange() {
    rebuildHistogram();
    if (DATA) renderSummary(DATA.stats);
  }

  // =================================================================
  // Controls wiring
  // =================================================================

  function getWeight() {
    return (
      document.querySelector('input[name="hist-weight"]:checked')?.value ||
      "count"
    );
  }
  function getCostUnit() {
    return (
      document.querySelector('input[name="cost-unit"]:checked')?.value || "muls"
    );
  }

  document.querySelectorAll('input[name="hist-weight"]').forEach((r) =>
    r.addEventListener("change", () => {
      updateControlVisibility();
      rebuildHistogram();
    }),
  );
  document
    .querySelectorAll('input[name="cost-unit"]')
    .forEach((r) => r.addEventListener("change", rebuildHistogram));
  document
    .getElementById("hist-group")
    .addEventListener("change", rebuildHistogram);

  function updateControlVisibility() {
    const isCost = getWeight() === "cost";
    document.getElementById("unit-group").classList.toggle("disabled", !isCost);
  }

  // =================================================================
  // Unified histogram render
  // =================================================================

  function rebuildHistogram() {
    if (!DATA) return;
    const weight = getWeight();
    const isCost = weight === "cost";
    const inBytes = isCost && getCostUnit() === "bytes";
    const unit = isCost ? (inBytes ? "Bytes" : "sVOLE") : "Count";

    const title = document.getElementById("hist-title");
    const subtitle = document.getElementById("hist-subtitle");
    title.textContent = `Operation ${unit === "Count" ? "Counts" : "Cost (" + unit + ")"}`;
    subtitle.textContent = isCost
      ? "Total cost of each instruction type across all executions"
      : "Number of times each instruction type was executed";
    renderOpsHistogram(DATA.stats, weight);
  }

  // =================================================================
  // Operations histogram
  // =================================================================

  const LOAD_OPS = new Set([
    "I32Load",
    "I64Load",
    "F32Load",
    "F64Load",
    "I32Load8S",
    "I32Load8U",
    "I32Load16S",
    "I32Load16U",
    "I64Load8S",
    "I64Load8U",
    "I64Load16S",
    "I64Load16U",
    "I64Load32S",
    "I64Load32U",
    "MixedLoadI32",
    "MixedLoadI64",
    "MixedLoadF32",
    "MixedLoadF64",
  ]);
  const STORE_OPS = new Set([
    "I32Store",
    "I64Store",
    "F32Store",
    "F64Store",
    "I32Store8",
    "I32Store16",
    "I64Store8",
    "I64Store16",
    "I64Store32",
  ]);
  const DATA_OPS = new Set(["Copy", "GlobalGet", "GlobalSet"]);

  function opCategory(name) {
    if (LOAD_OPS.has(name)) return "Memory";
    if (STORE_OPS.has(name)) return "Memory";
    if (DATA_OPS.has(name)) return "Memory";
    return "Arithmetic";
  }

  const categoryOrder = ["Arithmetic", "Memory"];

  function renderOpsHistogram(s, weight) {
    const container = document.getElementById("hist-content");
    const showPub = true;
    const showPriv = true;
    const grouped = document.getElementById("hist-group").checked;
    const isCost = weight === "cost";
    const inBytes = isCost && getCostUnit() === "bytes";
    const scale = inBytes ? 1.44 / 8 : 1;
    const unit = isCost ? (inBytes ? "Bytes" : "sVOLE") : "";
    const fmt = (n) => Math.ceil(n).toLocaleString();

    const allOps = new Map();
    if (showPub)
      for (const [k, v] of Object.entries(s.public_cf_histogram || {})) {
        const val = isCost ? getMuls(k, false) * v * scale : v;
        allOps.set(k, { public: val, private: 0 });
      }
    if (showPriv)
      for (const [k, v] of Object.entries(s.private_cf_histogram || {})) {
        const val = isCost ? getMuls(k, true) * v * scale : v;
        if (allOps.has(k)) allOps.get(k).private = val;
        else allOps.set(k, { public: 0, private: val });
      }

    const flat = [];
    for (const [name, counts] of allOps) {
      const total =
        (showPub ? counts.public : 0) + (showPriv ? counts.private : 0);
      if (total > 0) flat.push([name, counts, total]);
    }
    flat.sort((a, b) => b[2] - a[2]);

    let maxVal = flat.length > 0 ? flat[0][2] : 1;

    function renderRow(name, counts) {
      const total =
        (showPub ? counts.public : 0) + (showPriv ? counts.private : 0);
      const publicW = showPub ? ((counts.public / maxVal) * 100).toFixed(1) : 0;
      const privateW = showPriv
        ? ((counts.private / maxVal) * 100).toFixed(1)
        : 0;
      return `<div class="hist-row">
                <span class="hist-name">${name}</span>
                <span class="hist-bar-wrap">
                    ${showPub && counts.public > 0 ? `<span class="hist-bar public" style="width:${publicW}%"></span>` : ""}
                    ${showPriv && counts.private > 0 ? `<span class="hist-bar private" style="width:${privateW}%"></span>` : ""}
                    <span class="hist-count">${fmt(total)}</span>
                </span>
            </div>`;
    }

    let html = "";
    if (grouped) {
      const groups = new Map();
      for (const [name, counts, total] of flat) {
        const cat = opCategory(name);
        if (!groups.has(cat)) groups.set(cat, []);
        groups.get(cat).push([name, counts, total]);
      }
      for (const cat of categoryOrder) {
        const entries = groups.get(cat);
        if (!entries || entries.length === 0) continue;
        const groupTotal = entries.reduce((s, e) => s + e[2], 0);
        html += `<div class="hist-section"><h3>${cat} <span style="font-weight:400;color:#9e9e9e;">(${fmt(groupTotal)})</span></h3>`;
        for (const [name, counts] of entries) html += renderRow(name, counts);
        html += "</div>";
      }
    } else {
      for (const [name, counts] of flat) html += renderRow(name, counts);
    }
    container.innerHTML = html;
  }

  // Auto-load first profile
  selector.value = "json";
  selector.dispatchEvent(new Event("change"));
})();
