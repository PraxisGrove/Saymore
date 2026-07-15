const TARGET_SAMPLE_RATE = 16_000;

const elements = {
  caseList: document.querySelector("#case-list"),
  progressLabel: document.querySelector("#progress-label"),
  progressPercent: document.querySelector("#progress-percent"),
  progressFill: document.querySelector("#progress-fill"),
  caseCategory: document.querySelector("#case-category"),
  caseId: document.querySelector("#case-id"),
  caseTitle: document.querySelector("#case-title"),
  savedBadge: document.querySelector("#saved-badge"),
  readText: document.querySelector("#read-text"),
  expectedText: document.querySelector("#expected-text"),
  caseCheck: document.querySelector("#case-check"),
  meterFill: document.querySelector("#meter-fill"),
  recordingStatus: document.querySelector(".recording-status"),
  statusText: document.querySelector("#status-text"),
  timer: document.querySelector("#timer"),
  playback: document.querySelector("#playback"),
  recordButton: document.querySelector("#record-button"),
  stopButton: document.querySelector("#stop-button"),
  retryButton: document.querySelector("#retry-button"),
  saveButton: document.querySelector("#save-button"),
  previousButton: document.querySelector("#previous-button"),
  nextButton: document.querySelector("#next-button"),
  outputPath: document.querySelector("#output-path"),
  message: document.querySelector("#message"),
  viewTabs: [...document.querySelectorAll(".view-tab")],
  viewPanels: [...document.querySelectorAll(".view-panel")],
  providerState: document.querySelector("#provider-state"),
  evaluationCases: document.querySelector("#evaluation-cases"),
  toggleCasesButton: document.querySelector("#toggle-cases-button"),
  caseSelectionSummary: document.querySelector("#case-selection-summary"),
  asrOptions: document.querySelector("#asr-options"),
  llmOptions: document.querySelector("#llm-options"),
  hotwordsToggle: document.querySelector("#hotwords-toggle"),
  forceRefinementToggle: document.querySelector("#force-refinement-toggle"),
  remoteConsent: document.querySelector("#remote-consent"),
  startRunButton: document.querySelector("#start-run-button"),
  runMessage: document.querySelector("#run-message"),
  activeRun: document.querySelector("#active-run"),
  activeRunLabel: document.querySelector("#active-run-label"),
  activeRunCount: document.querySelector("#active-run-count"),
  activeRunId: document.querySelector("#active-run-id"),
  runProgressFill: document.querySelector("#run-progress-fill"),
  cancelRunButton: document.querySelector("#cancel-run-button"),
  runSelector: document.querySelector("#run-selector"),
  emptyResults: document.querySelector("#empty-results"),
  resultContent: document.querySelector("#result-content"),
  summaryMetrics: document.querySelector("#summary-metrics"),
  resultCaseTabs: document.querySelector("#result-case-tabs"),
  resultCaseMeta: document.querySelector("#result-case-meta"),
  comparisonGrid: document.querySelector("#comparison-grid"),
};

const state = {
  cases: [],
  recordings: {},
  index: 0,
  mode: "idle",
  chunks: [],
  inputSampleRate: TARGET_SAMPLE_RATE,
  startedAt: 0,
  durationMs: 0,
  clip: null,
  clipUrl: null,
  timerId: null,
  stream: null,
  audioContext: null,
  source: null,
  processor: null,
  silentGain: null,
  providers: { asr: [], llm: [] },
  selectedCases: new Set(),
  selectedAsr: null,
  selectedLlms: new Set(),
  runs: [],
  activeRunId: null,
  runPollId: null,
  displayedRun: null,
  displayedCaseIndex: 0,
};

await initialize();

async function initialize() {
  try {
    const [casesResponse, statusResponse] = await Promise.all([
      fetch("/cases.json"),
      fetch("/api/status"),
    ]);
    if (!casesResponse.ok || !statusResponse.ok) throw new Error("local data unavailable");
    state.cases = await casesResponse.json();
    state.recordings = (await statusResponse.json()).recordings;
    const firstIncomplete = state.cases.findIndex((item) => !state.recordings[item.id]);
    state.index = firstIncomplete === -1 ? 0 : firstIncomplete;
    Object.keys(state.recordings).forEach((caseId) => state.selectedCases.add(caseId));
    bindActions();
    renderAll();
    renderEvaluationCases();
    updateRunButton();
    void initializeWorkbench();
  } catch {
    showMessage("无法读取本地测试清单，请重新启动录音服务。", true);
  }
}

function bindActions() {
  elements.recordButton.addEventListener("click", startRecording);
  elements.stopButton.addEventListener("click", stopRecording);
  elements.retryButton.addEventListener("click", discardClip);
  elements.saveButton.addEventListener("click", saveAndAdvance);
  elements.previousButton.addEventListener("click", () => selectCase(state.index - 1));
  elements.nextButton.addEventListener("click", () => selectCase(state.index + 1));
  elements.viewTabs.forEach((tab) => tab.addEventListener("click", () => switchView(tab.dataset.view)));
  elements.toggleCasesButton.addEventListener("click", toggleRecordedCases);
  elements.remoteConsent.addEventListener("change", updateRunButton);
  elements.startRunButton.addEventListener("click", startEvaluationRun);
  elements.cancelRunButton.addEventListener("click", cancelEvaluationRun);
  elements.runSelector.addEventListener("change", () => loadRunResult(elements.runSelector.value));
}

function renderAll() {
  renderCaseList();
  renderCurrentCase();
  renderProgress();
  renderControls();
}

function renderCaseList() {
  elements.caseList.replaceChildren();
  let category = null;
  state.cases.forEach((item, index) => {
    if (item.category !== category) {
      category = item.category;
      const heading = document.createElement("div");
      heading.className = "category-heading";
      heading.textContent = category;
      elements.caseList.append(heading);
    }
    const button = document.createElement("button");
    button.type = "button";
    button.className = "case-item";
    button.classList.toggle("active", index === state.index);
    button.classList.toggle("saved", Boolean(state.recordings[item.id]));
    button.dataset.index = String(index);
    button.innerHTML = `
      <span class="case-item-id">${escapeHtml(item.id)}</span>
      <span class="case-item-title">${escapeHtml(item.title)}</span>
      <span class="case-item-state" aria-hidden="true"></span>
    `;
    button.setAttribute("aria-label", `${item.id} ${item.title}${state.recordings[item.id] ? "，已保存" : ""}`);
    button.addEventListener("click", () => selectCase(index));
    elements.caseList.append(button);
  });
}

function renderCurrentCase() {
  const item = currentCase();
  if (!item) return;
  const saved = state.recordings[item.id];
  elements.caseCategory.textContent = item.category;
  elements.caseId.textContent = item.id;
  elements.caseTitle.textContent = item.title;
  elements.readText.textContent = item.read;
  elements.expectedText.textContent = item.expected;
  elements.caseCheck.textContent = item.check;
  elements.savedBadge.hidden = !saved;
  elements.outputPath.textContent = `tests/voice-evaluation/recordings/${item.id}/recording.wav`;
  document.title = `${item.id} ${item.title} · Saymore 语音评测工作台`;
  if (!state.clip && saved) {
    elements.playback.src = `/api/recordings/${encodeURIComponent(item.id)}?v=${encodeURIComponent(saved.recordedAt)}`;
    elements.playback.hidden = false;
    state.durationMs = saved.durationMs;
  } else if (!state.clip) {
    elements.playback.removeAttribute("src");
    elements.playback.hidden = true;
    state.durationMs = 0;
  }
  updateTimer();
}

function renderProgress() {
  const completed = Object.keys(state.recordings).length;
  const percent = state.cases.length === 0 ? 0 : Math.round((completed / state.cases.length) * 100);
  elements.progressLabel.textContent = `${completed} / ${state.cases.length} 已完成`;
  elements.progressPercent.textContent = `${percent}%`;
  elements.progressFill.style.width = `${percent}%`;
}

function renderControls() {
  const recording = state.mode === "recording";
  const saving = state.mode === "saving";
  elements.recordButton.disabled = recording || saving;
  elements.stopButton.disabled = !recording;
  elements.retryButton.disabled = recording || saving || (!state.clip && !state.recordings[currentCase()?.id]);
  elements.saveButton.disabled = recording || saving || !state.clip;
  elements.previousButton.disabled = recording || saving || state.index === 0;
  elements.nextButton.disabled = recording || saving || state.index === state.cases.length - 1;
  elements.recordingStatus.classList.toggle("active", recording);
  elements.recordingStatus.classList.toggle("ready", state.mode === "review");
  if (recording) elements.statusText.textContent = "正在录音";
  else if (saving) elements.statusText.textContent = "正在保存";
  else if (state.clip) elements.statusText.textContent = "请试听后保存";
  else if (state.recordings[currentCase()?.id]) elements.statusText.textContent = "已保存，可重新录制";
  else elements.statusText.textContent = "准备录音";
}

async function startRecording() {
  if (state.mode === "recording") return;
  clearFreshClip();
  showMessage("");
  try {
    state.stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: false,
        noiseSuppression: false,
        autoGainControl: false,
      },
    });
    state.audioContext = new AudioContext();
    state.inputSampleRate = state.audioContext.sampleRate;
    state.source = state.audioContext.createMediaStreamSource(state.stream);
    state.processor = state.audioContext.createScriptProcessor(4096, 1, 1);
    state.silentGain = state.audioContext.createGain();
    state.silentGain.gain.value = 0;
    state.chunks = [];
    state.processor.onaudioprocess = captureAudio;
    state.source.connect(state.processor);
    state.processor.connect(state.silentGain);
    state.silentGain.connect(state.audioContext.destination);
    state.startedAt = performance.now();
    state.durationMs = 0;
    state.mode = "recording";
    state.timerId = window.setInterval(updateRecordingTime, 100);
    renderControls();
    updateTimer();
  } catch (error) {
    await releaseAudio();
    state.mode = "idle";
    renderControls();
    showMessage(microphoneErrorMessage(error), true);
  }
}

function captureAudio(event) {
  const samples = event.inputBuffer.getChannelData(0);
  state.chunks.push(new Float32Array(samples));
  let squareSum = 0;
  for (let index = 0; index < samples.length; index += 1) squareSum += samples[index] ** 2;
  const rms = Math.sqrt(squareSum / samples.length);
  elements.meterFill.style.width = `${Math.min(100, Math.max(2, rms * 360))}%`;
}

async function stopRecording() {
  if (state.mode !== "recording") return;
  state.durationMs = Math.round(performance.now() - state.startedAt);
  window.clearInterval(state.timerId);
  state.timerId = null;
  await releaseAudio();
  elements.meterFill.style.width = "0%";
  const merged = mergeChunks(state.chunks);
  const resampled = resample(merged, state.inputSampleRate, TARGET_SAMPLE_RATE);
  state.clip = encodeWav(resampled, TARGET_SAMPLE_RATE);
  state.clipUrl = URL.createObjectURL(state.clip);
  elements.playback.src = state.clipUrl;
  elements.playback.hidden = false;
  state.mode = "review";
  renderControls();
  updateTimer();
  showMessage(state.durationMs < 900 ? "录音较短，请试听确认是否完整。" : "录音仅保存在当前页面内存中，确认后再写入本地目录。", state.durationMs < 500);
}

async function releaseAudio() {
  if (state.processor) state.processor.onaudioprocess = null;
  state.source?.disconnect();
  state.processor?.disconnect();
  state.silentGain?.disconnect();
  state.stream?.getTracks().forEach((track) => track.stop());
  if (state.audioContext && state.audioContext.state !== "closed") await state.audioContext.close();
  state.stream = null;
  state.audioContext = null;
  state.source = null;
  state.processor = null;
  state.silentGain = null;
}

function discardClip() {
  clearFreshClip();
  state.mode = "idle";
  renderCurrentCase();
  renderControls();
  showMessage("可以重新录制当前条目。", false);
}

function clearFreshClip() {
  if (state.clipUrl) URL.revokeObjectURL(state.clipUrl);
  state.clip = null;
  state.clipUrl = null;
  state.chunks = [];
  elements.playback.pause();
  elements.playback.removeAttribute("src");
  elements.playback.hidden = true;
}

async function saveAndAdvance() {
  const item = currentCase();
  if (!item || !state.clip) return;
  state.mode = "saving";
  renderControls();
  showMessage("");
  try {
    const response = await fetch(`/api/recordings/${encodeURIComponent(item.id)}`, {
      method: "PUT",
      headers: {
        "Content-Type": "audio/wav",
        "X-Duration-Ms": String(state.durationMs),
      },
      body: state.clip,
    });
    if (!response.ok) throw new Error(`save failed: ${response.status}`);
    state.recordings[item.id] = {
      recordedAt: new Date().toISOString(),
      durationMs: state.durationMs,
      bytes: state.clip.size,
    };
    clearFreshClip();
    state.mode = "idle";
    renderProgress();
    renderCaseList();
    state.selectedCases.add(item.id);
    renderEvaluationCases();
    updateRunButton();
    const next = nextIncompleteIndex(state.index);
    if (next === null) {
      renderCurrentCase();
      renderControls();
      showMessage("23 条录音已经全部保存。", false);
      return;
    }
    selectCase(next);
    showMessage(`${item.id} 已保存，继续录制下一条。`, false);
  } catch {
    state.mode = "review";
    renderControls();
    showMessage("保存失败，录音仍保留在当前页面，请检查本地服务。", true);
  }
}

function nextIncompleteIndex(currentIndex) {
  for (let offset = 1; offset <= state.cases.length; offset += 1) {
    const index = (currentIndex + offset) % state.cases.length;
    if (!state.recordings[state.cases[index].id]) return index;
  }
  return null;
}

function selectCase(index) {
  if (state.mode === "recording" || state.mode === "saving") return;
  if (index < 0 || index >= state.cases.length || index === state.index) return;
  clearFreshClip();
  state.index = index;
  state.mode = "idle";
  showMessage("");
  renderAll();
  document.querySelector(`.case-item[data-index="${index}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function currentCase() {
  return state.cases[state.index];
}

function updateRecordingTime() {
  state.durationMs = Math.round(performance.now() - state.startedAt);
  updateTimer();
}

function updateTimer() {
  const totalTenths = Math.floor(state.durationMs / 100);
  const minutes = Math.floor(totalTenths / 600);
  const seconds = Math.floor((totalTenths % 600) / 10);
  const tenths = totalTenths % 10;
  elements.timer.textContent = `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}.${tenths}`;
}

function mergeChunks(chunks) {
  const length = chunks.reduce((total, chunk) => total + chunk.length, 0);
  const merged = new Float32Array(length);
  let offset = 0;
  chunks.forEach((chunk) => {
    merged.set(chunk, offset);
    offset += chunk.length;
  });
  return merged;
}

function resample(input, sourceRate, targetRate) {
  if (sourceRate === targetRate) return input;
  const outputLength = Math.max(1, Math.round(input.length * targetRate / sourceRate));
  const output = new Float32Array(outputLength);
  const ratio = sourceRate / targetRate;
  for (let index = 0; index < outputLength; index += 1) {
    const position = index * ratio;
    const before = Math.floor(position);
    const after = Math.min(before + 1, input.length - 1);
    const fraction = position - before;
    output[index] = input[before] * (1 - fraction) + input[after] * fraction;
  }
  return output;
}

function encodeWav(samples, sampleRate) {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  writeAscii(view, 0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  writeAscii(view, 8, "WAVE");
  writeAscii(view, 12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeAscii(view, 36, "data");
  view.setUint32(40, samples.length * 2, true);
  for (let index = 0; index < samples.length; index += 1) {
    const sample = Math.max(-1, Math.min(1, samples[index]));
    view.setInt16(44 + index * 2, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
  }
  return new Blob([buffer], { type: "audio/wav" });
}

function writeAscii(view, offset, value) {
  for (let index = 0; index < value.length; index += 1) view.setUint8(offset + index, value.charCodeAt(index));
}

function microphoneErrorMessage(error) {
  if (error?.name === "NotAllowedError") return "麦克风权限未开启，请在浏览器设置中允许本机页面使用麦克风。";
  if (error?.name === "NotFoundError") return "没有检测到可用麦克风。";
  return "无法启动麦克风，请关闭其他占用录音设备的应用后重试。";
}

function showMessage(message, error = false) {
  elements.message.textContent = message;
  elements.message.classList.toggle("error", error);
}

function escapeHtml(value) {
  return value.replace(/[&<>"]/g, (character) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[character]);
}

async function initializeWorkbench() {
  try {
    const [providersResponse, runsResponse] = await Promise.all([
      fetch("/api/providers"),
      fetch("/api/runs"),
    ]);
    if (!providersResponse.ok || !runsResponse.ok) throw new Error("workbench unavailable");
    state.providers = await providersResponse.json();
    state.runs = (await runsResponse.json()).runs;
    state.selectedAsr = state.providers.asr.find((provider) => provider.configured)?.id ?? null;
    state.providers.llm
      .filter((provider) => provider.configured && provider.consented)
      .forEach((provider) => state.selectedLlms.add(provider.id));
    elements.providerState.textContent = `开发环境 · ${state.providers.asr.length} 个 ASR · ${state.providers.llm.length} 个 LLM`;
    elements.providerState.className = "provider-state ready";
    renderProviderOptions();
    renderRunSelector();
    updateRunButton();
    const running = state.runs.find((run) => run.state === "running");
    if (running) {
      state.activeRunId = running.runId;
      startRunPolling();
    }
    if (!document.querySelector("#results-view").hidden && !state.displayedRun) {
      loadLatestCompletedRun();
    }
  } catch {
    elements.providerState.textContent = "无法读取开发环境 Provider 配置";
    elements.providerState.className = "provider-state error";
    showRunMessage("请确认本地服务从 Saymore 仓库根目录启动。", true);
  }
}

function switchView(view) {
  elements.viewTabs.forEach((tab) => tab.classList.toggle("active", tab.dataset.view === view));
  elements.viewPanels.forEach((panel) => { panel.hidden = panel.id !== `${view}-view`; });
  document.title = "Saymore 语音评测工作台";
  if (view === "results" && !state.displayedRun) {
    loadLatestCompletedRun();
  }
}

function loadLatestCompletedRun() {
  const completed = state.runs.find((run) => run.state === "completed");
  if (completed) void loadRunResult(completed.runId);
}

function renderEvaluationCases() {
  if (!state.cases.length) return;
  elements.evaluationCases.replaceChildren();
  state.cases.forEach((item) => {
    const recorded = Boolean(state.recordings[item.id]);
    const label = document.createElement("label");
    label.className = "case-checkbox";
    const input = document.createElement("input");
    input.type = "checkbox";
    input.disabled = !recorded;
    input.checked = recorded && state.selectedCases.has(item.id);
    input.addEventListener("change", () => {
      if (input.checked) state.selectedCases.add(item.id);
      else state.selectedCases.delete(item.id);
      renderCaseSelectionSummary();
      updateRunButton();
    });
    const id = document.createElement("strong");
    id.textContent = item.id;
    const title = document.createElement("span");
    title.textContent = recorded ? item.title : "未录制";
    label.append(input, id, title);
    elements.evaluationCases.append(label);
  });
  renderCaseSelectionSummary();
}

function renderCaseSelectionSummary() {
  const recorded = Object.keys(state.recordings).length;
  elements.caseSelectionSummary.textContent = `已选择 ${state.selectedCases.size} 条，共 ${recorded} 条可用录音；未录制条目不会进入评测。`;
  const allSelected = recorded > 0 && state.selectedCases.size === recorded;
  elements.toggleCasesButton.textContent = allSelected ? "清除选择" : "全选已录制";
}

function toggleRecordedCases() {
  const recordedIds = Object.keys(state.recordings);
  const allSelected = recordedIds.length > 0 && recordedIds.every((id) => state.selectedCases.has(id));
  state.selectedCases.clear();
  if (!allSelected) recordedIds.forEach((id) => state.selectedCases.add(id));
  renderEvaluationCases();
  updateRunButton();
}

function renderProviderOptions() {
  elements.asrOptions.replaceChildren();
  state.providers.asr.forEach((provider) => {
    const option = providerOption(provider, "radio", state.selectedAsr === provider.id);
    option.input.name = "asr-provider";
    option.input.addEventListener("change", () => {
      state.selectedAsr = provider.id;
      updateRunButton();
    });
    elements.asrOptions.append(option.label);
  });
  elements.llmOptions.replaceChildren();
  state.providers.llm.forEach((provider) => {
    const ready = provider.configured && provider.consented;
    const option = providerOption(provider, "checkbox", state.selectedLlms.has(provider.id));
    option.input.disabled = !ready;
    option.input.addEventListener("change", () => {
      if (option.input.checked) state.selectedLlms.add(provider.id);
      else state.selectedLlms.delete(provider.id);
      updateRunButton();
    });
    elements.llmOptions.append(option.label);
  });
}

function providerOption(provider, type, checked) {
  const label = document.createElement("label");
  label.className = "provider-option";
  const input = document.createElement("input");
  input.type = type;
  input.checked = checked;
  input.disabled = !provider.configured;
  const copy = document.createElement("span");
  const name = document.createElement("strong");
  name.textContent = provider.name;
  const model = document.createElement("small");
  model.textContent = provider.model || "尚未选择模型";
  copy.append(name, model);
  const status = document.createElement("em");
  status.textContent = provider.configured ? provider.consented || type === "radio" ? "可用" : "待确认" : "未配置";
  label.append(input, copy, status);
  return { label, input };
}

function updateRunButton() {
  const ready = state.selectedCases.size > 0
    && Boolean(state.selectedAsr)
    && state.selectedLlms.size > 0
    && elements.remoteConsent.checked
    && !state.activeRunId;
  elements.startRunButton.disabled = !ready;
}

async function startEvaluationRun() {
  showRunMessage("");
  elements.startRunButton.disabled = true;
  try {
    const response = await fetch("/api/runs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        caseIds: [...state.selectedCases],
        asrProviderId: state.selectedAsr,
        llmProviderIds: [...state.selectedLlms],
        hotwordsEnabled: elements.hotwordsToggle.checked,
        forceRefinement: elements.forceRefinementToggle.checked,
        confirmRemote: elements.remoteConsent.checked,
      }),
    });
    const value = await response.json();
    if (!response.ok) throw new Error(value.error ?? "run_start_failed");
    state.activeRunId = value.runId;
    elements.remoteConsent.checked = false;
    showRunMessage("评测已经在本机后台启动。", false);
    startRunPolling();
  } catch (error) {
    showRunMessage(runErrorMessage(error.message), true);
    state.activeRunId = null;
    updateRunButton();
  }
}

function startRunPolling() {
  window.clearInterval(state.runPollId);
  elements.activeRun.hidden = false;
  elements.cancelRunButton.hidden = false;
  elements.activeRunId.textContent = state.activeRunId;
  void pollActiveRun();
  state.runPollId = window.setInterval(pollActiveRun, 1000);
  updateRunButton();
}

async function pollActiveRun() {
  if (!state.activeRunId) return;
  try {
    const response = await fetch(`/api/runs/${encodeURIComponent(state.activeRunId)}`);
    if (!response.ok) throw new Error("run_status_failed");
    const value = await response.json();
    const progress = value.progress ?? { completed: 0, total: value.status.caseCount ?? 0, current_case: null };
    const percent = progress.total ? Math.round(progress.completed / progress.total * 100) : 0;
    const phase = ({ asr: "语音识别", llm: "模型润色" })[progress.phase] ?? "评测";
    elements.activeRunLabel.textContent = progress.current_case ? `${progress.current_case} · ${phase}` : runStateLabel(value.status.state);
    elements.activeRunCount.textContent = `${progress.completed} / ${progress.total}`;
    elements.runProgressFill.style.width = `${percent}%`;
    if (value.status.state !== "running") {
      window.clearInterval(state.runPollId);
      state.runPollId = null;
      const completedRunId = state.activeRunId;
      state.activeRunId = null;
      elements.cancelRunButton.hidden = true;
      await refreshRuns();
      updateRunButton();
      if (value.status.state === "completed" && value.result) {
        showRunMessage("评测完成，已生成模型对比结果。", false);
        state.displayedRun = value;
        renderResult(value);
        switchView("results");
        elements.runSelector.value = completedRunId;
      } else {
        showRunMessage(value.status.state === "cancelled" ? "评测已取消。" : "评测失败，请查看本地运行日志。", value.status.state !== "cancelled");
      }
    }
  } catch {
    showRunMessage("暂时无法读取后台评测进度。", true);
  }
}

async function cancelEvaluationRun() {
  if (!state.activeRunId) return;
  const response = await fetch(`/api/runs/${encodeURIComponent(state.activeRunId)}/cancel`, { method: "POST" });
  if (!response.ok) showRunMessage("当前运行无法取消，可能已经结束。", true);
}

async function refreshRuns() {
  const response = await fetch("/api/runs");
  if (!response.ok) return;
  state.runs = (await response.json()).runs;
  renderRunSelector();
}

function showRunMessage(message, error = false) {
  elements.runMessage.textContent = message;
  elements.runMessage.classList.toggle("error", error);
}

function runErrorMessage(error) {
  const messages = {
    remote_confirmation_required: "请先确认远程数据发送范围。",
    incomplete_run_selection: "请选择录音、ASR 和至少一个润色模型。",
    case_not_recorded: "选择中包含尚未录制的条目。",
    provider_not_ready: "所选 Provider 尚未配置或未完成数据确认。",
  };
  return messages[error] ?? "无法启动批量评测，请检查本地配置。";
}

function runStateLabel(runState) {
  return ({ running: "正在评测", completed: "评测完成", failed: "评测失败", cancelled: "已取消" })[runState] ?? runState;
}

function renderRunSelector() {
  const completed = state.runs.filter((run) => run.state === "completed");
  elements.runSelector.replaceChildren();
  completed.forEach((run) => {
    const option = document.createElement("option");
    option.value = run.runId;
    option.textContent = `${formatRunDate(run.startedAt)} · ${run.caseCount} 条 · ${run.llms.map((item) => item.name).join(" / ")}`;
    elements.runSelector.append(option);
  });
  elements.runSelector.disabled = completed.length === 0;
}

async function loadRunResult(runId) {
  if (!runId) return;
  const response = await fetch(`/api/runs/${encodeURIComponent(runId)}`);
  if (!response.ok) return;
  const value = await response.json();
  if (!value.result) return;
  state.displayedRun = value;
  state.displayedCaseIndex = 0;
  renderResult(value);
}

function renderResult(value) {
  const result = value.result;
  elements.emptyResults.hidden = true;
  elements.resultContent.hidden = false;
  renderSummaryMetrics(result);
  elements.resultCaseTabs.replaceChildren();
  result.cases.forEach((item, index) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "result-case-tab";
    button.classList.toggle("active", index === state.displayedCaseIndex);
    button.textContent = item.case_id;
    button.addEventListener("click", () => {
      state.displayedCaseIndex = index;
      renderResult(value);
    });
    elements.resultCaseTabs.append(button);
  });
  const item = result.cases[state.displayedCaseIndex];
  elements.resultCaseMeta.textContent = `${item.category} · ${item.title}`;
  renderComparison(item);
}

function renderSummaryMetrics(result) {
  const splitMetrics = result.cases.some((item) => Number.isFinite(item.asr.content_character_error_rate));
  const asrScores = result.cases.map((item) => contentCer(item.asr)).filter(Number.isFinite);
  const llmIds = [...new Set(result.cases.flatMap((item) => item.llm.map((entry) => entry.provider_id)))];
  const metrics = [
    { label: "测试条目", value: String(result.cases.length), detail: `${result.dictionary_terms} 个本地热词` },
    {
      label: splitMetrics ? "ASR 内容 CER" : "ASR 旧版 CER",
      value: formatPercent(average(asrScores)),
      detail: splitMetrics ? `${result.cases[0]?.asr.provider_name ?? ""} · 对照忠实逐字稿` : "旧运行尚未拆分内容与标点",
    },
  ];
  llmIds.slice(0, 2).forEach((providerId) => {
    const entries = result.cases.flatMap((item) => item.llm).filter((entry) => entry.provider_id === providerId);
    const ruleScores = entries.map((entry) => entry.rule_pass_rate).filter(Number.isFinite);
    metrics.push({
      label: `${entries[0]?.provider_name ?? "LLM"} ${splitMetrics ? "内容 CER" : "旧版 CER"}`,
      value: formatPercent(average(entries.map(contentCer).filter(Number.isFinite))),
      detail: ruleScores.length
        ? `规则通过 ${formatPercent(average(ruleScores))} · ${entries.filter((entry) => entry.exact_match).length} 条精确匹配`
        : `${entries.filter((entry) => entry.exact_match).length} 条精确匹配`,
    });
  });
  elements.summaryMetrics.replaceChildren(...metrics.map(metricElement));
}

function metricElement(metric) {
  const element = document.createElement("div");
  element.className = "metric";
  const label = document.createElement("span");
  label.textContent = metric.label;
  const value = document.createElement("strong");
  value.textContent = metric.value;
  const detail = document.createElement("small");
  detail.textContent = metric.detail;
  element.append(label, value, detail);
  return element;
}

function renderComparison(item) {
  const splitMetrics = Number.isFinite(item.asr.content_character_error_rate);
  const cerLabel = splitMetrics ? "内容 CER" : "旧版 CER";
  const columns = [
    {
      name: "忠实逐字稿",
      detail: item.asr_reference ? "ASR reference" : "旧运行未单独保存",
      text: item.asr_reference ?? item.expected,
      score: item.asr_reference ? "ASR 内容基准" : "仅兼容展示",
    },
    { name: "测试预期", detail: "仅用于固定评测", text: item.expected, score: "回归测试参考" },
    {
      name: item.asr.provider_name,
      detail: `${item.asr.model} · ${item.asr.duration_ms} ms`,
      text: item.asr.transcript ?? item.asr.error ?? "没有结果",
      score: `${cerLabel} ${formatPercent(contentCer(item.asr))} · 标点 ${formatPercent(item.asr.punctuation_score)}`,
    },
    ...(item.local ? [{
      name: "本地词条纠正",
      detail: `${item.local.matched_terms.length} 个词条命中 · ${item.local.duration_us} us`,
      text: item.local.text,
      score: item.local.exact_match
        ? "精确匹配"
        : `${cerLabel} ${formatPercent(contentCer(item.local))} · 标点 ${formatPercent(item.local.punctuation_score)} · 确定性处理`,
      rules: item.local.rule_results ?? [],
    }] : []),
    ...item.llm.map((entry) => {
      const ruleScore = Number.isFinite(entry.rule_pass_rate) ? ` · 规则 ${formatPercent(entry.rule_pass_rate)}` : "";
      const structure = typeof entry.structure_match === "boolean" ? ` · 结构${entry.structure_match ? "通过" : "未通过"}` : "";
      return {
        name: entry.provider_name,
        detail: `${entry.model} · ${entry.duration_ms} ms`,
        text: entry.text ?? entry.error ?? "没有结果",
        score: entry.exact_match
          ? `精确匹配${ruleScore}`
          : `${cerLabel} ${formatPercent(contentCer(entry))} · 标点 ${formatPercent(entry.punctuation_score)}${structure}${ruleScore} · ${statusLabel(entry.status)}`,
        candidate: entry.provider_output && entry.provider_output !== entry.text ? entry.provider_output : null,
        rules: entry.rule_results ?? [],
      };
    }),
  ];
  elements.comparisonGrid.style.setProperty("--comparison-columns", String(columns.length));
  elements.comparisonGrid.replaceChildren(...columns.map(comparisonColumn));
}

function comparisonColumn(column) {
  const element = document.createElement("section");
  element.className = "comparison-column";
  const header = document.createElement("header");
  const name = document.createElement("strong");
  name.textContent = column.name;
  const detail = document.createElement("span");
  detail.textContent = column.detail;
  header.append(name, detail);
  const text = document.createElement("p");
  text.className = "comparison-text";
  text.textContent = column.text;
  const score = document.createElement("div");
  score.className = "score-line";
  const label = document.createElement("span");
  label.textContent = "结果";
  const value = document.createElement("strong");
  value.textContent = column.score;
  score.append(label, value);
  element.append(header, text);
  if (column.candidate) {
    const candidate = document.createElement("div");
    candidate.className = "candidate-output";
    const candidateLabel = document.createElement("strong");
    candidateLabel.textContent = "守卫前候选";
    const candidateText = document.createElement("p");
    candidateText.textContent = column.candidate;
    candidate.append(candidateLabel, candidateText);
    element.append(candidate);
  }
  if (column.rules?.length) {
    const rules = document.createElement("div");
    rules.className = "rule-results";
    column.rules.forEach((rule) => {
      const badge = document.createElement("span");
      badge.className = rule.passed ? "passed" : "failed";
      badge.textContent = `${rule.passed ? "通过" : "失败"} · ${rule.label}`;
      rules.append(badge);
    });
    element.append(rules);
  }
  element.append(score);
  return element;
}

function contentCer(entry) {
  return entry.content_character_error_rate ?? entry.character_error_rate;
}

function average(values) {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null;
}

function formatPercent(value) {
  return Number.isFinite(value) ? `${(value * 100).toFixed(1)}%` : "—";
}

function formatRunDate(value) {
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : date.toLocaleString("zh-CN", { hour12: false });
}

function statusLabel(status) {
  const labels = {
    completed: "已润色",
    skipped_short: "短文本旁路",
    fallback_output_rejected: "输出被拒绝",
    fallback_timeout: "超时回退",
  };
  return labels[status] ?? status.replaceAll("_", " ");
}
