"use strict";

const $ = (selector) => document.querySelector(selector);
const $$ = (selector) => [...document.querySelectorAll(selector)];
const state = {
  range: "24h",
  charts: {},
  started: null,
  refreshing: false,
  running: false,
};
const colors = {
  grid: "rgba(112,145,133,.12)",
  text: "#6f857d",
  download: "#67e8b7",
  upload: "#55cce8",
  latency: "#a98cf4",
  jitter: "#ef8fce",
  loss: "#e9be6f",
};

Chart.defaults.color = colors.text;
Chart.defaults.font.family = "Inter, system-ui, sans-serif";
Chart.defaults.font.size = 10;

function number(value, digits = 1) {
  return value == null || !Number.isFinite(Number(value))
    ? "—"
    : Number(value).toFixed(digits);
}

function metric(selector, value, unit, digits = 1, signed = false) {
  const prefix = signed && Number(value) > 0 ? "+" : "";
  $(selector).innerHTML =
    value == null
      ? "—"
      : `${prefix}${number(value, digits)}<span>${unit}</span>`;
}

function localTime(value, short = false) {
  if (!value) return "—";
  const date = new Date(value);
  return short
    ? date.toLocaleString([], {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : date.toLocaleString([], { dateStyle: "medium", timeStyle: "medium" });
}

function relative(value) {
  if (!value) return "Never";
  const seconds = Math.max(0, (Date.now() - new Date(value)) / 1000);
  if (seconds < 60) return "Just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} hr ago`;
  return `${Math.floor(seconds / 86400)} days ago`;
}

async function request(url, options) {
  const response = await fetch(url, options);
  let data = {};
  try {
    data = await response.json();
  } catch {}
  if (!response.ok)
    throw new Error(data.error || `Request failed (${response.status})`);
  return data;
}

function notify(message) {
  const toast = $("#toast");
  toast.textContent = message;
  toast.hidden = false;
  clearTimeout(notify.timer);
  notify.timer = setTimeout(() => {
    toast.hidden = true;
  }, 5000);
}

async function loadSummary() {
  const summary = await request("/api/summary");
  metric("#metric-download", summary.latest_download_mbps, "Mbps");
  metric("#metric-upload", summary.latest_upload_mbps, "Mbps");
  metric("#metric-latency", summary.latest_latency_ms, "ms");
  metric("#metric-jitter", summary.latest_jitter_ms, "ms");
  metric("#metric-bufferbloat", summary.latest_bufferbloat_ms, "ms", 1, true);
  metric("#metric-loss", summary.latest_packet_loss_pct, "%", 2);
  const down = summary.latest_download_bufferbloat_ms;
  const up = summary.latest_upload_bufferbloat_ms;
  $("#bufferbloat-detail").textContent =
    down == null && up == null
      ? "ms · worst added loaded latency"
      : `download ${
          down == null ? "—" : `${down >= 0 ? "+" : ""}${number(down)} ms`
        } · upload ${
          up == null ? "—" : `${up >= 0 ? "+" : ""}${number(up)} ms`
        }`;
  $("#last-completed").textContent = summary.latest?.completed_at
    ? relative(summary.latest.completed_at)
    : "No results yet";
  const alert = $("#connection-alert");
  if (summary.latest?.outcome === "unavailable") {
    $("#connection-alert-detail").textContent =
      summary.latest.outcome_detail ||
      "No valid samples were received from the configured measurement targets.";
    alert.hidden = false;
  } else {
    alert.hidden = true;
  }
}

function updateVisibleCount(name, chart) {
  const minimum = Number(chart.scales.x.min);
  const maximum = Number(chart.scales.x.max);
  const points = chart.data.datasets.flatMap((dataset) => dataset.data);
  const measured = points.filter((point) => point.y != null && Number.isFinite(Number(point.y)));
  const visible = measured.filter((point) => point.x >= minimum && point.x <= maximum).length;
  const label = $(`#${name}-count`);
  if (label) {
    label.textContent =
      visible < measured.length
        ? `· showing ${visible} of ${measured.length}`
        : `· ${measured.length} point${measured.length === 1 ? "" : "s"}`;
  }
}

function setZoomed(name, chart) {
  const button = $(`.reset-zoom[data-chart="${name}"]`);
  if (button) button.disabled = false;
  updateVisibleCount(name, chart);
}

function chartOptions(name, yTitle, suggestedMax) {
  return {
    responsive: true,
    maintainAspectRatio: false,
    interaction: { mode: "nearest", intersect: false },
    animation: { duration: 250 },
    parsing: false,
    plugins: {
      legend: { display: false },
      tooltip: {
        backgroundColor: "#15241f",
        borderColor: "#315044",
        borderWidth: 1,
        padding: 10,
        titleColor: "#dce9e4",
        bodyColor: "#a9bbb4",
        displayColors: true,
        callbacks: {
          title: (items) => (items.length ? localTime(items[0].parsed.x) : ""),
        },
      },
      zoom: {
        limits: { x: { min: "original", max: "original", minRange: 1 } },
        pan: {
          enabled: true,
          mode: "x",
          threshold: 5,
          onPanComplete: ({ chart }) => setZoomed(name, chart),
        },
        zoom: {
          mode: "x",
          wheel: { enabled: true, speed: 0.3 },
          drag: {
            enabled: true,
            modifierKey: "shift",
            borderColor: colors.download,
            borderWidth: 1,
            backgroundColor: "rgba(103,232,183,.12)",
          },
          pinch: { enabled: true },
          onZoomComplete: ({ chart }) => setZoomed(name, chart),
        },
      },
    },
    scales: {
      x: {
        type: "linear",
        grid: { display: false },
        border: { display: false },
        ticks: {
          autoSkip: true,
          maxTicksLimit: name === "throughput" ? 12 : 6,
          maxRotation: 0,
          callback: (value) => localTime(Number(value), true),
        },
      },
      y: {
        beginAtZero: true,
        suggestedMax,
        grid: { color: colors.grid },
        border: { display: false },
        title: { display: true, text: yTitle, color: colors.text },
        ticks: { padding: 8 },
      },
    },
  };
}

function line(label, data, color, pointCount, fill = true) {
  return {
    label,
    data,
    borderColor: color,
    backgroundColor: `${color}16`,
    borderWidth: 2,
    pointRadius: pointCount <= 30 ? 3 : 1.5,
    pointHoverRadius: 5,
    tension: 0.25,
    spanGaps: false,
    fill,
  };
}

function selected(run, test) {
  if (run.selected_tests) return Boolean(run.selected_tests[test]);
  if (run.test_kind === "quality") return test === "latency" || test === "packet_loss";
  return true;
}

function seriesPoints(runs, field, test) {
  return runs
    .filter((run) => selected(run, test))
    .map((run) => ({
      x: new Date(run.started_at).getTime(),
      y:
        run.status === "completed" && run.outcome !== "unavailable"
          ? run[field]
          : null,
    }));
}

function createChart(
  name,
  canvas,
  datasets,
  yTitle,
  emptySelector,
  countSelector,
  suggestedMax,
) {
  const count = datasets.reduce(
    (maximum, dataset) =>
      Math.max(maximum, dataset.data.filter((point) => point.y != null).length),
    0,
  );
  $(emptySelector).hidden = count > 0;
  $(countSelector).textContent = count
    ? `· ${count} point${count === 1 ? "" : "s"}`
    : "";
  if (state.charts[name]) state.charts[name].destroy();
  const reset = $(`.reset-zoom[data-chart="${name}"]`);
  if (reset) reset.disabled = true;
  state.charts[name] = new Chart($(canvas), {
    type: "line",
    data: { datasets },
    options: chartOptions(name, yTitle, suggestedMax),
  });
}

function renderCharts(runs) {
  const chronological = [...runs].reverse();
  const pointCount = chronological.length;
  createChart(
    "throughput",
    "#throughput-chart",
    [
      line("Download", seriesPoints(chronological, "download_mbps", "download"), colors.download, pointCount),
      line("Upload", seriesPoints(chronological, "upload_mbps", "upload"), colors.upload, pointCount),
    ],
    "Mbps",
    "#throughput-empty",
    "#throughput-count",
  );
  createChart(
    "latency",
    "#latency-chart",
    [
      line("Idle", seriesPoints(chronological, "latency_ms", "latency"), colors.latency, pointCount),
      line("Download loaded", seriesPoints(chronological, "loaded_latency_download_ms", "download"), colors.download, pointCount, false),
      line("Upload loaded", seriesPoints(chronological, "loaded_latency_upload_ms", "upload"), colors.upload, pointCount, false),
    ],
    "Milliseconds",
    "#latency-empty",
    "#latency-count",
  );
  createChart(
    "bufferbloat",
    "#bufferbloat-chart",
    [
      line("Download added latency", seriesPoints(chronological, "download_bufferbloat_ms", "download"), colors.download, pointCount),
      line("Upload added latency", seriesPoints(chronological, "upload_bufferbloat_ms", "upload"), colors.upload, pointCount),
    ],
    "Added latency (ms)",
    "#bufferbloat-empty",
    "#bufferbloat-count",
  );
  createChart(
    "jitter",
    "#jitter-chart",
    [line("Jitter", seriesPoints(chronological, "jitter_ms", "latency"), colors.jitter, pointCount)],
    "Milliseconds",
    "#jitter-empty",
    "#jitter-count",
  );
  createChart(
    "loss",
    "#loss-chart",
    [line("Packet loss", seriesPoints(chronological, "packet_loss_pct", "packet_loss"), colors.loss, pointCount)],
    "Loss %",
    "#loss-empty",
    "#loss-count",
    1,
  );
}

function setLivePoint(chartName, datasetIndex, startedAt, value) {
  if (value == null) return;
  const chart = state.charts[chartName];
  if (!chart) return;
  const dataset = chart.data.datasets[datasetIndex];
  const x = new Date(startedAt).getTime();
  dataset.data = dataset.data.filter((point) => !point.live);
  dataset.data.push({ x, y: value, live: true });
  dataset.data.sort((left, right) => left.x - right.x);
  chart.update("none");
}

function renderPartial(active) {
  const partial = active.partial || {};
  const completed = partial.completed || {};
  const cards = [
    ["latency", "#metric-latency", ".metric-card.latency", partial.latency_ms, "ms", "latency", 0],
    ["latency", "#metric-jitter", ".metric-card.jitter", partial.jitter_ms, "ms", "jitter", 0],
    ["download", "#metric-download", ".metric-card.download", partial.download_mbps, "Mbps", "throughput", 0],
    ["upload", "#metric-upload", ".metric-card.upload", partial.upload_mbps, "Mbps", "throughput", 1],
    ["packet_loss", "#metric-loss", ".metric-card.loss", partial.packet_loss_pct, "%", "loss", 0],
  ];
  cards.forEach(([test, selector, cardSelector, value, unit, chart, dataset]) => {
    if (!completed[test] || value == null) return;
    metric(selector, value, unit, unit === "%" ? 2 : 1);
    $(cardSelector)?.classList.add("current-run");
    setLivePoint(chart, dataset, active.started_at, value);
  });
  if (completed.download && partial.loaded_latency_download_ms != null) {
    setLivePoint(
      "latency",
      1,
      active.started_at,
      partial.loaded_latency_download_ms,
    );
  }
  if (completed.upload && partial.loaded_latency_upload_ms != null) {
    setLivePoint(
      "latency",
      2,
      active.started_at,
      partial.loaded_latency_upload_ms,
    );
  }
  if (completed.download && partial.download_bufferbloat_ms != null) {
    metric("#metric-bufferbloat", partial.download_bufferbloat_ms, "ms", 1, true);
    $(".metric-card.bufferbloat")?.classList.add("current-run");
    setLivePoint("bufferbloat", 0, active.started_at, partial.download_bufferbloat_ms);
  }
  if (completed.upload && partial.upload_bufferbloat_ms != null) {
    const current = Math.max(
      partial.download_bufferbloat_ms ?? 0,
      partial.upload_bufferbloat_ms,
    );
    metric("#metric-bufferbloat", current, "ms", 1, true);
    $(".metric-card.bufferbloat")?.classList.add("current-run");
    setLivePoint("bufferbloat", 1, active.started_at, partial.upload_bufferbloat_ms);
  }
  if (completed.download || completed.upload) {
    const down = partial.download_bufferbloat_ms;
    const up = partial.upload_bufferbloat_ms;
    $("#bufferbloat-detail").textContent = `download ${
      down == null ? "—" : `+${number(down)} ms`
    } · upload ${up == null ? "—" : `+${number(up)} ms`}`;
  }
}

function selectionLabel(run) {
  const selection = run.selected_tests;
  if (!selection) return `${run.trigger} · ${run.test_kind}`;
  const names = [];
  if (selection.latency) names.push("latency");
  if (selection.download) names.push("download");
  if (selection.upload) names.push("upload");
  if (selection.packet_loss) names.push("loss");
  return `${run.trigger} · ${names.join(", ")}`;
}

function bufferbloatCell(run) {
  const down = run.download_bufferbloat_ms;
  const up = run.upload_bufferbloat_ms;
  const format = (value) =>
    value == null ? "—" : `${value >= 0 ? "+" : ""}${number(value)}`;
  return `${format(down)} / ${format(up)} ms`;
}

function renderTable(runs) {
  $("#run-count").textContent = `${runs.length} run${
    runs.length === 1 ? "" : "s"
  }`;
  $("#runs-body").innerHTML = runs.length
    ? runs
        .slice(0, 100)
        .map(
          (run) => `<tr>
    <td>${localTime(run.started_at)}</td>
    <td><span class="trigger">${escapeHtml(selectionLabel(run))}</span></td>
    <td>${
      run.download_mbps == null ? "—" : `${number(run.download_mbps)} Mbps`
    }</td>
    <td>${
      run.upload_mbps == null ? "—" : `${number(run.upload_mbps)} Mbps`
    }</td>
    <td>${run.latency_ms == null ? "—" : `${number(run.latency_ms)} ms`}</td>
    <td>${run.jitter_ms == null ? "—" : `${number(run.jitter_ms)} ms`}</td>
    <td>${bufferbloatCell(run)}</td>
    <td>${
      run.packet_loss_pct == null ? "—" : `${number(run.packet_loss_pct, 2)}%`
    }</td>
    <td><span class="status-badge ${
      run.outcome === "unavailable" ? "unavailable" : run.status
    }">${run.outcome === "unavailable" ? "unavailable" : run.status}</span>${
      run.error || run.outcome_detail
        ? `<span class="error-detail" title="${escapeHtml(
            run.error || run.outcome_detail,
          )}">${escapeHtml(run.error || run.outcome_detail)}</span>`
        : ""
    }</td>
  </tr>`,
        )
        .join("")
    : `<tr><td colspan="9" class="table-empty">No diagnostics in this range. Run a test to create the first result.</td></tr>`;
}

function escapeHtml(value) {
  const node = document.createElement("div");
  node.textContent = value;
  return node.innerHTML;
}

async function loadHistory() {
  if (state.refreshing) return;
  state.refreshing = true;
  $("#history-error").hidden = true;
  try {
    const data = await request(`/api/results?range=${state.range}&limit=1000`);
    renderCharts(data.results);
    renderTable(data.results);
  } catch (error) {
    $("#history-error").textContent = error.message;
    $("#history-error").hidden = false;
  } finally {
    state.refreshing = false;
  }
}

async function loadCurrent() {
  try {
    const [current, health] = await Promise.all([
      request("/api/current"),
      request("/healthz"),
    ]);
    const pill = $("#status-pill");
    const panel = $("#running-panel");
    state.running = current.running;
    pill.className = `status-pill ${current.running ? "running" : ""}`;
    pill.querySelector("span").textContent = current.running
      ? "Running"
      : "Healthy";
    pill.title = current.running
      ? "A diagnostic is running"
      : `Healthcheck ready · database ${
          health.database ? "ready" : "unavailable"
        } · scheduler ${health.scheduler ? "ready" : "unavailable"}`;
    updateRunButton();
    panel.hidden = !current.running;
    if (current.running && current.active_test) {
      state.started = current.active_test.started_at;
      $("#running-phase").textContent = humanPhase(current.active_test.phase);
      renderPartial(current.active_test);
    } else if (state.started) {
      $$(".metric-card.current-run").forEach((card) =>
        card.classList.remove("current-run"),
      );
      state.started = null;
      await Promise.all([loadSummary(), loadHistory()]);
    }
  } catch {
    state.running = false;
    const pill = $("#status-pill");
    pill.className = "status-pill error";
    pill.querySelector("span").textContent = "Unavailable";
    pill.title = "The API or readiness healthcheck could not be reached";
    updateRunButton();
  }
}

function humanPhase(value) {
  return value
    .replace(/_/g, " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function tickElapsed() {
  if (!state.started) return;
  const seconds = Math.max(
    0,
    Math.floor((Date.now() - new Date(state.started)) / 1000),
  );
  $("#elapsed").textContent = `${String(Math.floor(seconds / 60)).padStart(
    2,
    "0",
  )}:${String(seconds % 60).padStart(2, "0")}`;
}

function selectedTests() {
  const values = new Set(
    $$('input[name="diagnostic"]:checked').map((input) => input.value),
  );
  return {
    latency: values.has("latency"),
    download: values.has("download"),
    upload: values.has("upload"),
    packet_loss: values.has("packet_loss"),
  };
}

function updateRunButton() {
  const count = $$('input[name="diagnostic"]:checked').length;
  const button = $("#run-button");
  button.disabled = state.running || count === 0;
  button.querySelector("span:last-child").textContent = state.running
    ? "Running…"
    : count === 4
      ? "Run all tests"
      : count === 0
        ? "Select a test"
        : `Run ${count} selected`;
  $("#run-options-button").disabled = state.running;
}

async function runNow() {
  const selection = selectedTests();
  const button = $("#run-button");
  button.disabled = true;
  $("#run-options").hidden = true;
  $("#run-options-button").setAttribute("aria-expanded", "false");
  try {
    await request("/api/run", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(selection),
    });
    await loadCurrent();
  } catch (error) {
    notify(error.message);
    updateRunButton();
  }
}

$$(".range-control button").forEach((button) =>
  button.addEventListener("click", () => {
    $$(".range-control button").forEach((item) =>
      item.classList.remove("active"),
    );
    button.classList.add("active");
    state.range = button.dataset.range;
    loadHistory();
  }),
);
$$(".reset-zoom").forEach((button) =>
  button.addEventListener("click", () => {
    const chart = state.charts[button.dataset.chart];
    chart?.resetZoom();
    if (chart) updateVisibleCount(button.dataset.chart, chart);
    button.disabled = true;
  }),
);
$$(".zoom-step").forEach((button) =>
  button.addEventListener("click", () => {
    const chart = state.charts[button.dataset.chart];
    if (!chart) return;
    chart.zoom(Number(button.dataset.factor));
    setZoomed(button.dataset.chart, chart);
  }),
);
$$('input[name="diagnostic"]').forEach((input) =>
  input.addEventListener("change", updateRunButton),
);
$("#select-all-tests").addEventListener("click", () => {
  $$('input[name="diagnostic"]').forEach((input) => {
    input.checked = true;
  });
  updateRunButton();
});
$("#run-options-button").addEventListener("click", (event) => {
  event.stopPropagation();
  const menu = $("#run-options");
  menu.hidden = !menu.hidden;
  $("#run-options-button").setAttribute("aria-expanded", String(!menu.hidden));
});
$("#run-options").addEventListener("click", (event) => event.stopPropagation());
document.addEventListener("click", () => {
  $("#run-options").hidden = true;
  $("#run-options-button").setAttribute("aria-expanded", "false");
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    $("#run-options").hidden = true;
    $("#run-options-button").setAttribute("aria-expanded", "false");
  }
});
$("#run-button").addEventListener("click", runNow);

updateRunButton();
Promise.all([loadSummary(), loadHistory(), loadCurrent()]).catch((error) =>
  notify(error.message),
);
setInterval(loadCurrent, 2000);
setInterval(tickElapsed, 1000);
setInterval(loadSummary, 30000);
