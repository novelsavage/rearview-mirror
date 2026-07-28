import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { load, type Store } from "@tauri-apps/plugin-store";
import "./styles.css";

type Settings = {
  cameraId: string;
  mirrored: boolean;
  size: number;
  sizeDefaultVersion: number;
  moveEnabled: boolean;
  position?: { x: number; y: number };
};

const DEFAULT_SETTINGS: Settings = {
  cameraId: "",
  mirrored: true,
  size: 180,
  sizeDefaultVersion: 2,
  moveEnabled: true,
};

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
  // v0.1.0 の初期値 600px を使っていた試作版だけ、新しい小型初期値へ移行する。
  if (!savedSettings?.sizeDefaultVersion && savedSettings?.size === 600) {
    settings.size = DEFAULT_SETTINGS.size;
  }
  await saveSettings();
}

function stopCamera(): void {
  activeStream?.getTracks().forEach((track) => track.stop());
  activeStream = undefined;
  const video = document.querySelector<HTMLVideoElement>("#mirror-video");
  if (video) video.srcObject = null;
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
  const video = document.querySelector<HTMLVideoElement>("#mirror-video")!;
  video.classList.toggle("is-mirrored", settings.mirrored);

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
      <header>
        <p class="eyebrow">REARVIEW MIRROR</p>
        <h1>設定</h1>
        <p class="description">ミラー本体には映像以外を表示しません。表示は <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Space</kbd> を押している間だけです。</p>
      </header>
      <section>
        <h2>カメラ</h2>
        <label>使用するカメラ
          <select id="camera-select"><option value="">標準のカメラ</option></select>
        </label>
        <button id="grant-camera" type="button">カメラを確認・許可する</button>
        <p class="hint">音声は取得しません。映像の保存や送信も行いません。</p>
      </section>
      <section>
        <h2>ミラー</h2>
        <label>長辺 <output id="size-value"></output> px
          <input id="size-range" type="range" min="120" max="1000" step="10" />
        </label>
        <div class="presets" aria-label="サイズプリセット">
          <button type="button" data-size="120">120</button>
          <button type="button" data-size="180">180</button>
          <button type="button" data-size="240">240</button>
          <button type="button" data-size="320">320</button>
        </div>
        <label class="check"><input id="mirror-toggle" type="checkbox" /> 左右を反転する</label>
        <label class="check"><input id="move-toggle" type="checkbox" /> ショートカット中のマウス移動で位置を変える</label>
        <button id="reset-position" class="secondary" type="button">位置を右上に戻す</button>
      </section>
      <footer>設定はタスクトレイのRearview Mirrorアイコンからいつでも開けます。</footer>
    </main>`;

  const cameraSelect = document.querySelector<HTMLSelectElement>("#camera-select")!;
  const sizeRange = document.querySelector<HTMLInputElement>("#size-range")!;
  const sizeValue = document.querySelector<HTMLOutputElement>("#size-value")!;
  const mirrorToggle = document.querySelector<HTMLInputElement>("#mirror-toggle")!;
  const moveToggle = document.querySelector<HTMLInputElement>("#move-toggle")!;

  const syncForm = (): void => {
    sizeRange.value = String(settings.size);
    sizeValue.value = String(settings.size);
    mirrorToggle.checked = settings.mirrored;
    moveToggle.checked = settings.moveEnabled;
    cameraSelect.value = settings.cameraId;
  };

  const applySize = async (size: number): Promise<void> => {
    settings.size = size;
    sizeValue.value = String(size);
    sizeRange.value = String(size);
    await invoke("set_mirror_size", { longestEdge: size });
    await saveSettings();
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
  });
  sizeRange.addEventListener("input", () => void applySize(Number(sizeRange.value)));
  document.querySelectorAll<HTMLButtonElement>("[data-size]").forEach((button) => {
    button.addEventListener("click", () => void applySize(Number(button.dataset.size)));
  });
  mirrorToggle.addEventListener("change", async () => {
    settings.mirrored = mirrorToggle.checked;
    await saveSettings();
  });
  moveToggle.addEventListener("change", async () => {
    settings.moveEnabled = moveToggle.checked;
    await invoke("set_move_enabled", { enabled: settings.moveEnabled });
    await saveSettings();
  });
  document.querySelector("#reset-position")?.addEventListener("click", async () => {
    settings.position = undefined;
    await invoke("set_mirror_position", { x: window.screen.availWidth - settings.size - 24, y: 24 });
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
    renderMirror();
  } else {
    renderSettings();
  }
}

void main();
