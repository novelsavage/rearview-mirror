import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { load, type Store } from "@tauri-apps/plugin-store";
import "./styles.css";

type Settings = {
  cameraId: string;
  mirrored: boolean;
  grayscale: boolean;
  size: number;
  sizeDefaultVersion: number;
  layoutVersion: number;
  moveEnabled: boolean;
  toggleMode: boolean;
  position?: { x: number; y: number };
};

const DEFAULT_SETTINGS: Settings = {
  cameraId: "",
  mirrored: true,
  grayscale: false,
  // 初回だけ実機のタスクバー高から上書きするため、バージョンは未適用としておく。
  size: 240,
  sizeDefaultVersion: 0,
  layoutVersion: 0,
  moveEnabled: true,
  toggleMode: true,
};

const SIZE_DEFAULT_VERSION = 4;
const LAYOUT_VERSION = 7;

let store: Store;
let activeStream: MediaStream | undefined;
let settings: Settings = { ...DEFAULT_SETTINGS };

async function saveSettings(): Promise<void> {
  await store.set("settings", settings);
  await store.save();
}

async function loadSettings(): Promise<void> {
  store = await load("rearview-mirror.json", {
    autoSave: 150,
    defaults: { settings: DEFAULT_SETTINGS },
  });
  const savedSettings = await store.get<Partial<Settings>>("settings");
  settings = {
    ...DEFAULT_SETTINGS,
    ...savedSettings,
  };
  // 試作版の固定サイズから、タスクバー高に合わせた横長ミラーへ移行する。
  if ((savedSettings?.sizeDefaultVersion ?? 0) < SIZE_DEFAULT_VERSION) {
    settings.size = await invoke<number>("get_taskbar_mirror_size");
    settings.position = await invoke<{ x: number; y: number }>("get_taskbar_mirror_position", {
      width: settings.size,
    });
    settings.sizeDefaultVersion = SIZE_DEFAULT_VERSION;
  }
  // タスクバーの中に配置していた試作版だけ、タスクバー脇の安全な位置へ移行する。
  if ((savedSettings?.layoutVersion ?? 0) < LAYOUT_VERSION) {
    settings.position = await invoke<{ x: number; y: number }>("get_taskbar_mirror_position", {
      width: settings.size,
    });
    settings.layoutVersion = LAYOUT_VERSION;
  }
  await saveSettings();
}

function stopCamera(): void {
  activeStream?.getTracks().forEach((track) => track.stop());
  activeStream = undefined;
  const video = document.querySelector<HTMLVideoElement>("#mirror-video");
  if (video) video.srcObject = null;
}

function applyVisualSettings(): void {
  const video = document.querySelector<HTMLVideoElement>("#mirror-video");
  video?.classList.toggle("is-mirrored", settings.mirrored);
  video?.classList.toggle("is-grayscale", settings.grayscale);
}

async function notifyMirrorSettingsChanged(): Promise<void> {
  await invoke("set_display_options", {
    options: { mirrored: settings.mirrored, grayscale: settings.grayscale },
  });
  await emitTo("mirror", "mirror:settings-changed", settings);
}

async function startCamera(): Promise<void> {
  if (activeStream) return;
  const video = document.querySelector<HTMLVideoElement>("#mirror-video");
  if (!video) return;

  activeStream = await navigator.mediaDevices.getUserMedia({
    audio: false,
    video: {
      deviceId: settings.cameraId ? { exact: settings.cameraId } : undefined,
      width: { ideal: 1280 },
      height: { ideal: 720 },
      frameRate: { ideal: 15, max: 30 },
    },
  });
  video.srcObject = activeStream;
  await video.play();
}

function renderMirror(): void {
  document.body.innerHTML = '<video id="mirror-video" autoplay playsinline aria-label="後方確認用カメラ映像"></video>';
  applyVisualSettings();

  void listen("mirror:show", async () => {
    try {
      await startCamera();
    } catch (error) {
      console.error("カメラを開始できませんでした", error);
    }
  });

  void listen("mirror:hide", async () => {
    stopCamera();
    await getCurrentWindow().hide();
  });

  void listen<{ x: number; y: number }>("mirror:position", async (event) => {
    settings.position = event.payload;
    await saveSettings();
  });

  void listen<Settings>("mirror:settings-changed", async (event) => {
    const shouldRestartCamera = settings.cameraId !== event.payload.cameraId && Boolean(activeStream);
    settings = { ...settings, ...event.payload };
    applyVisualSettings();
    if (shouldRestartCamera) {
      stopCamera();
      try {
        await startCamera();
      } catch (error) {
        console.error("カメラを切り替えられませんでした", error);
      }
    }
  });

  void listen<{ mirrored: boolean; grayscale: boolean; move_enabled: boolean; toggle_mode: boolean }>("mirror:tray-options", (event) => {
    settings.mirrored = event.payload.mirrored;
    settings.grayscale = event.payload.grayscale;
    settings.moveEnabled = event.payload.move_enabled;
    settings.toggleMode = event.payload.toggle_mode;
    applyVisualSettings();
  });

  window.addEventListener("keydown", async (event) => {
    if (event.key === "Escape") {
      stopCamera();
      await getCurrentWindow().hide();
    }
  });
}

function renderSettings(): void {
  document.body.innerHTML = `
    <main class="settings">
      <header><h1>Rearview Mirror</h1><p><kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Space</kbd> で表示します。</p></header>
      <fieldset>
        <legend>カメラ</legend>
        <label>使用するカメラ
          <select id="camera-select"><option value="">標準のカメラ</option></select>
        </label>
        <button id="grant-camera" type="button">カメラを確認・許可する</button>
        <p class="hint">音声は取得しません。映像の保存や送信も行いません。</p>
      </fieldset>
      <fieldset>
        <legend>ミラー</legend>
        <label>長辺 <output id="size-value"></output> px
          <input id="size-range" type="range" min="128" max="1600" step="10" />
        </label>
        <div class="presets" aria-label="サイズプリセット">
          <button type="button" data-size="128">128</button>
          <button type="button" data-size="192">192</button>
          <button type="button" data-size="256">256</button>
          <button type="button" data-size="320">320</button>
        </div>
        <label class="check"><input id="mirror-toggle" type="checkbox" /> 左右を反転する</label>
        <label class="check"><input id="grayscale-toggle" type="checkbox" /> 白黒で表示する</label>
        <label class="check"><input id="move-toggle" type="checkbox" /> ショートカット中のマウス移動で位置を変える</label>
        <label class="check"><input id="toggle-mode" type="checkbox" /> ショートカットを切替表示にする</label>
        <button id="reset-position" class="secondary" type="button">位置をタスクバー脇に戻す</button>
      </fieldset>
      <footer>普段の切替はタスクトレイから行えます。</footer>
    </main>`;

  const cameraSelect = document.querySelector<HTMLSelectElement>("#camera-select")!;
  const sizeRange = document.querySelector<HTMLInputElement>("#size-range")!;
  const sizeValue = document.querySelector<HTMLOutputElement>("#size-value")!;
  const mirrorToggle = document.querySelector<HTMLInputElement>("#mirror-toggle")!;
  const grayscaleToggle = document.querySelector<HTMLInputElement>("#grayscale-toggle")!;
  const moveToggle = document.querySelector<HTMLInputElement>("#move-toggle")!;
  const toggleMode = document.querySelector<HTMLInputElement>("#toggle-mode")!;

  const syncForm = (): void => {
    sizeRange.value = String(settings.size);
    sizeValue.value = String(settings.size);
    mirrorToggle.checked = settings.mirrored;
    grayscaleToggle.checked = settings.grayscale;
    moveToggle.checked = settings.moveEnabled;
    toggleMode.checked = settings.toggleMode;
    cameraSelect.value = settings.cameraId;
  };

  const applySize = async (size: number): Promise<void> => {
    settings.size = size;
    sizeValue.value = String(size);
    sizeRange.value = String(size);
    await invoke("set_mirror_size", { longestEdge: size });
    await saveSettings();
    await notifyMirrorSettingsChanged();
  };

  const refreshCameras = async (): Promise<void> => {
    const devices = await navigator.mediaDevices.enumerateDevices();
    const cameras = devices.filter((device) => device.kind === "videoinput");
    cameraSelect.replaceChildren(new Option("標準のカメラ", ""));
    cameras.forEach((camera, index) => {
      cameraSelect.add(new Option(camera.label || `カメラ ${index + 1}`, camera.deviceId));
    });
    cameraSelect.value = settings.cameraId;
  };

  document.querySelector("#grant-camera")?.addEventListener("click", async () => {
    const stream = await navigator.mediaDevices.getUserMedia({ audio: false, video: true });
    stream.getTracks().forEach((track) => track.stop());
    await refreshCameras();
  });
  cameraSelect.addEventListener("change", async () => {
    settings.cameraId = cameraSelect.value;
    await saveSettings();
    await notifyMirrorSettingsChanged();
  });
  sizeRange.addEventListener("input", () => void applySize(Number(sizeRange.value)));
  document.querySelectorAll<HTMLButtonElement>("[data-size]").forEach((button) => {
    button.addEventListener("click", () => void applySize(Number(button.dataset.size)));
  });
  mirrorToggle.addEventListener("change", async () => {
    settings.mirrored = mirrorToggle.checked;
    await saveSettings();
    await notifyMirrorSettingsChanged();
  });
  grayscaleToggle.addEventListener("change", async () => {
    settings.grayscale = grayscaleToggle.checked;
    await saveSettings();
    await notifyMirrorSettingsChanged();
  });
  moveToggle.addEventListener("change", async () => {
    settings.moveEnabled = moveToggle.checked;
    await invoke("set_move_enabled", { enabled: settings.moveEnabled });
    await saveSettings();
    await notifyMirrorSettingsChanged();
  });
  toggleMode.addEventListener("change", async () => {
    settings.toggleMode = toggleMode.checked;
    await invoke("set_toggle_mode", { enabled: settings.toggleMode });
    await saveSettings();
  });
  document.querySelector("#reset-position")?.addEventListener("click", async () => {
    settings.position = await invoke<{ x: number; y: number }>("get_taskbar_mirror_position", {
      width: settings.size,
    });
    await invoke("set_mirror_position", settings.position);
    await saveSettings();
  });

  void listen<{ mirrored: boolean; grayscale: boolean; move_enabled: boolean; toggle_mode: boolean }>("settings:tray-options", async (event) => {
    settings.mirrored = event.payload.mirrored;
    settings.grayscale = event.payload.grayscale;
    settings.moveEnabled = event.payload.move_enabled;
    settings.toggleMode = event.payload.toggle_mode;
    syncForm();
    await saveSettings();
  });

  syncForm();
  void refreshCameras();
  void listen("settings:opened", () => void refreshCameras());
}

async function main(): Promise<void> {
  await loadSettings();
  const label = getCurrentWindow().label;
  if (label === "mirror") {
    if (settings.position) {
      await invoke("set_mirror_position", settings.position);
    }
    await invoke("set_mirror_size", { longestEdge: settings.size });
    await invoke("set_move_enabled", { enabled: settings.moveEnabled });
    await invoke("set_toggle_mode", { enabled: settings.toggleMode });
    await notifyMirrorSettingsChanged();
    renderMirror();
  } else {
    renderSettings();
  }
}

void main();
