import { ChangeEvent, CSSProperties, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { FaGithub } from "react-icons/fa";

type SettingsPayload = {
  amplitude_threshold: number;
  freq_ratio_threshold: number;
  cooldown_secs: number;
  active_pack: string;
  playback_mode: string;
  selected_sound: string | null;
};

type ConfigResponse = {
  settings: SettingsPayload;
  slap_count: number;
  sound_packs: string[];
  custom_packs: string[];
  active_pack_sounds: string[];
  active_pack_is_custom: boolean;
  mic_error: string | null;
  autolaunch_enabled: boolean;
};

type AppSnapshot = {
  settings: SettingsPayload;
  slap_count: number;
  sound_packs: string[];
  custom_packs: string[];
  active_pack_sounds: string[];
  active_pack_is_custom: boolean;
  mic_error: string | null;
};

type ImportSoundPayload = {
  file_name: string;
  bytes: number[];
};

const defaultConfig: ConfigResponse = {
  settings: {
    amplitude_threshold: 0.08,
    freq_ratio_threshold: 1.15,
    cooldown_secs: 0.55,
    active_pack: "classic",
    playback_mode: "cycle",
    selected_sound: null,
  },
  slap_count: 0,
  sound_packs: ["classic", "angry"],
  custom_packs: [],
  active_pack_sounds: [],
  active_pack_is_custom: false,
  mic_error: null,
  autolaunch_enabled: false,
};

const DONATION_UPI_QR_SRC = "/qr.png";
const DONATION_MEME_SRC = "/poor.gif";
const REPO_URL = "https://github.com/ad1tyayadav/slapWindows";

const globalStyles = `
  * {
    font-family: 'Courier New', 'Lucida Console', monospace;
    border-radius: 0 !important;
    box-sizing: border-box;
  }

  html,
  body,
  #root {
    margin: 0;
    min-height: 100%;
    background: #0a0f0a;
    color: #c8ffcc;
  }

  body {
    padding: 16px;
    user-select: none;
  }

  button,
  input {
    font: inherit;
  }

  button {
    appearance: none;
  }

  input[type=range] {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 6px;
    background: #122012;
    border: 1px solid #122012;
    outline: none;
    cursor: pointer;
    margin: 0;
  }

  input[type=range]::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 14px;
    height: 14px;
    background: #3d5f3d;
    border: none;
    border-radius: 0;
    cursor: pointer;
  }

  input[type=range]::-moz-range-thumb {
    width: 14px;
    height: 14px;
    background: #3d5f3d;
    border: none;
    border-radius: 0;
    cursor: pointer;
  }

  input[type=range]::-moz-range-track {
    height: 6px;
    background: #122012;
    border: 1px solid #122012;
  }

  .terminal-button,
  .pack-arrow,
  .toggle-button {
    transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
  }

  .terminal-button:hover:not(:disabled),
  .pack-arrow:hover:not(:disabled),
  .toggle-button:hover:not(:disabled) {
    background: #22ff44 !important;
    color: #0a0f0a !important;
    border-color: #22ff44 !important;
  }

  .danger-button:hover:not(:disabled) {
    background: #ff4444 !important;
    color: #0a0f0a !important;
    border-color: #ff4444 !important;
  }

  .mode-button.active,
  .sound-button.active {
    background: #22ff44 !important;
    color: #0a0f0a !important;
    border-color: #22ff44 !important;
  }

  .guide-overlay {
    position: fixed;
    inset: 0;
    background: rgba(10, 15, 10, 0.88);
    display: grid;
    place-items: center;
    padding: 16px;
    z-index: 999;
  }

  .guide-modal {
    width: min(100%, 520px);
    background: #0f1a0f;
    border: 1px solid #22ff44;
  }

  .donation-modal {
    width: min(100%, 420px);
    background: #0f1a0f;
    border: 1px solid #22ff44;
  }

  button:disabled {
    cursor: wait !important;
    opacity: 1;
  }

  *::-webkit-scrollbar {
    width: 8px;
    height: 8px;
  }

  *::-webkit-scrollbar-track {
    background: #0a0f0a;
    border: 1px solid #1a3a1a;
  }

  *::-webkit-scrollbar-thumb {
    background: #122012;
    border: 1px solid #3d5f3d;
  }
`;

function formatValue(value: number) {
  return value.toFixed(2);
}

function displayPackName(pack: string) {
  return pack === "custom" ? "DEFAULT" : pack.toUpperCase();
}

function sliderFill(value: number, min: number, max: number): CSSProperties {
  const percentage = ((value - min) / (max - min)) * 100;
  return {
    background: `linear-gradient(90deg, #3d5f3d 0%, #3d5f3d ${percentage}%, #122012 ${percentage}%, #122012 100%)`,
    border: "1px solid #122012",
  };
}

function App() {
  const [config, setConfig] = useState<ConfigResponse>(defaultConfig);
  const [sensitivity, setSensitivity] = useState(defaultConfig.settings.amplitude_threshold);
  const [slapFilter, setSlapFilter] = useState(defaultConfig.settings.freq_ratio_threshold);
  const [cooldown, setCooldown] = useState(defaultConfig.settings.cooldown_secs);
  const [micName, setMicName] = useState("unknown device");
  const [status, setStatus] = useState("SYSTEM READY");
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [showGuide, setShowGuide] = useState(false);
  const [showDonate, setShowDonate] = useState(false);
  const [qrMissing, setQrMissing] = useState(false);
  const [memeMissing, setMemeMissing] = useState(false);
  const [ready, setReady] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const folderInputRef = useRef<HTMLInputElement | null>(null);
  const skipSliderSyncRef = useRef(true);
  const configRef = useRef(defaultConfig);

  const applyConfig = (next: ConfigResponse | AppSnapshot) => {
    const current = configRef.current;
    const merged: ConfigResponse = {
      ...current,
      ...next,
      settings: next.settings,
      slap_count: next.slap_count,
      sound_packs: next.sound_packs,
      custom_packs: next.custom_packs,
      active_pack_sounds: next.active_pack_sounds,
      active_pack_is_custom: next.active_pack_is_custom,
      mic_error: next.mic_error,
      autolaunch_enabled: "autolaunch_enabled" in next ? next.autolaunch_enabled : current.autolaunch_enabled,
    };

    skipSliderSyncRef.current = true;
    configRef.current = merged;
    setConfig(merged);
    setSensitivity(merged.settings.amplitude_threshold);
    setSlapFilter(merged.settings.freq_ratio_threshold);
    setCooldown(merged.settings.cooldown_secs);
    queueMicrotask(() => {
      skipSliderSyncRef.current = false;
    });
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    const initialize = async () => {
      const [initialConfig, detectedMicName] = await Promise.all([
        invoke<ConfigResponse>("get_config"),
        invoke<string>("get_mic_name").catch(() => "unknown device"),
      ]);

      applyConfig(initialConfig);
      setMicName(detectedMicName);
      setReady(true);

      unlisten = await listen<AppSnapshot>("slap-state", (event) => {
        applyConfig(event.payload);
      });
    };

    initialize().catch((error) => {
      setStatus(`BOOT ERROR: ${String(error).toUpperCase()}`);
      setReady(true);
      skipSliderSyncRef.current = false;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!ready || skipSliderSyncRef.current) {
      return;
    }

    const timeout = window.setTimeout(async () => {
      try {
        const next = await invoke<ConfigResponse>("update_config", {
          key: "amplitude_threshold",
          value: sensitivity,
        });
        applyConfig(next);
      } catch (error) {
        setStatus(`SENSITIVITY ERROR: ${String(error).toUpperCase()}`);
      }
    }, 150);

    return () => window.clearTimeout(timeout);
  }, [sensitivity, ready]);

  useEffect(() => {
    if (!ready || skipSliderSyncRef.current) {
      return;
    }

    const timeout = window.setTimeout(async () => {
      try {
        const next = await invoke<ConfigResponse>("update_config", {
          key: "freq_ratio_threshold",
          value: slapFilter,
        });
        applyConfig(next);
      } catch (error) {
        setStatus(`FILTER ERROR: ${String(error).toUpperCase()}`);
      }
    }, 150);

    return () => window.clearTimeout(timeout);
  }, [slapFilter, ready]);

  useEffect(() => {
    if (!ready || skipSliderSyncRef.current) {
      return;
    }

    const timeout = window.setTimeout(async () => {
      try {
        const next = await invoke<ConfigResponse>("update_config", {
          key: "cooldown_secs",
          value: cooldown,
        });
        applyConfig(next);
      } catch (error) {
        setStatus(`COOLDOWN ERROR: ${String(error).toUpperCase()}`);
      }
    }, 150);

    return () => window.clearTimeout(timeout);
  }, [cooldown, ready]);

  const HIDDEN_PACKS = new Set(["classic", "angry", "custom"]);
  const visiblePacks = config.sound_packs.filter((p: string) => !HIDDEN_PACKS.has(p));
  const currentPackIndex = visiblePacks.findIndex((pack) => pack === config.settings.active_pack);
  const activeStatus = config.mic_error ? "INACTIVE" : "ACTIVE";
  const activeStatusColor = config.mic_error ? "#5a8a5a" : "#22ff44";

  const updatePlaybackSettings = async (patch: Partial<SettingsPayload>, busy: string, successMessage: string) => {
    setBusyKey(busy);

    try {
      const nextSettings: SettingsPayload = {
        ...config.settings,
        ...patch,
      };

      const next = await invoke<AppSnapshot>("set_settings", { settings: nextSettings });
      applyConfig(next);
      setStatus(successMessage);
    } catch (error) {
      setStatus(`PLAYBACK ERROR: ${String(error).toUpperCase()}`);
    } finally {
      setBusyKey(null);
    }
  };

  const cyclePack = async (direction: number) => {
    if (visiblePacks.length === 0) {
      return;
    }

    setBusyKey("pack");

    try {
      const nextIndex = currentPackIndex < 0
        ? (direction > 0 ? 0 : visiblePacks.length - 1)
        : (currentPackIndex + direction + visiblePacks.length) % visiblePacks.length;
      const nextPack = visiblePacks[nextIndex];
      const next = await invoke<ConfigResponse>("set_sound_pack", { pack: nextPack });
      applyConfig(next);
      setStatus(`PACK SET: ${displayPackName(nextPack)}`);
    } catch (error) {
      setStatus(`PACK ERROR: ${String(error).toUpperCase()}`);
    } finally {
      setBusyKey(null);
    }
  };

  const runTestSound = async () => {
    setBusyKey("test");

    try {
      await invoke("test_sound");
      setStatus("TEST SOUND OK");
    } catch (error) {
      setStatus(`TEST ERROR: ${String(error).toUpperCase()}`);
    } finally {
      setBusyKey(null);
    }
  };

  const resetSlapCount = async () => {
    setBusyKey("reset");

    try {
      const next = await invoke<ConfigResponse>("reset_slap_count");
      applyConfig(next);
      setStatus("SLAP COUNT RESET");
    } catch (error) {
      setStatus(`RESET ERROR: ${String(error).toUpperCase()}`);
    } finally {
      setBusyKey(null);
    }
  };

  const toggleAutolaunch = async () => {
    setBusyKey("autolaunch");

    try {
      const enabled = await invoke<boolean>("toggle_autolaunch");
      configRef.current = { ...configRef.current, autolaunch_enabled: enabled };
      setConfig((current) => ({ ...current, autolaunch_enabled: enabled }));
      setStatus(enabled ? "AUTO-LAUNCH ON" : "AUTO-LAUNCH OFF");
    } catch (error) {
      setStatus(`AUTO-LAUNCH ERROR: ${String(error).toUpperCase()}`);
    } finally {
      setBusyKey(null);
    }
  };

  const openFilePicker = () => {
    fileInputRef.current?.click();
  };

  const openFolderPicker = () => {
    folderInputRef.current?.click();
  };

  const removeSound = async (fileName: string) => {
    setBusyKey(`remove-${fileName}`);

    try {
      const snapshot = await invoke<AppSnapshot>("remove_custom_sound", {
        packName: config.settings.active_pack,
        fileName,
      });
      applyConfig(snapshot);
      setStatus(`REMOVED ${fileName.toUpperCase()}`);
    } catch (error) {
      setStatus(`REMOVE ERROR: ${String(error).toUpperCase()}`);
    } finally {
      setBusyKey(null);
    }
  };

  const handleCustomSoundImport = async (event: ChangeEvent<HTMLInputElement>) => {
    const input = event.currentTarget;
    const selectedFiles = Array.from(input.files ?? []);
    if (selectedFiles.length === 0) {
      input.value = "";
      return;
    }

    setBusyKey("bonus-files");

    try {
      const files: ImportSoundPayload[] = await Promise.all(
        selectedFiles.map(async (file) => ({
          file_name: file.name,
          bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
        })),
      );

      const targetPack = config.active_pack_is_custom ? config.settings.active_pack : "custom";
      const snapshot = await invoke<AppSnapshot>("import_custom_sounds", {
        packName: targetPack,
        files,
      });
      applyConfig(snapshot);
      setStatus(`FILES ADDED TO ${displayPackName(targetPack)}`);
    } catch (error) {
      setStatus(`IMPORT ERROR: ${String(error).toUpperCase()}`);
    } finally {
      input.value = "";
      setBusyKey(null);
    }
  };

  const handleFolderImport = async (event: ChangeEvent<HTMLInputElement>) => {
    const input = event.currentTarget;
    const selectedFiles = Array.from(input.files ?? []);
    if (selectedFiles.length === 0) {
      input.value = "";
      return;
    }

    setBusyKey("bonus-folder");

    try {
      const filesWithPaths = selectedFiles.filter((file) => Boolean((file as File & { webkitRelativePath?: string }).webkitRelativePath));
      const firstPath = (filesWithPaths[0] as File & { webkitRelativePath?: string } | undefined)?.webkitRelativePath ?? "";
      const packName = firstPath.split("/")[0]?.trim() || "custom-folder";
      const files: ImportSoundPayload[] = await Promise.all(
        selectedFiles.map(async (file) => ({
          file_name: file.name,
          bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
        })),
      );

      const snapshot = await invoke<AppSnapshot>("import_custom_sounds", {
        packName,
        files,
      });
      applyConfig(snapshot);
      setStatus(`NEW PACK IMPORTED: ${displayPackName(packName)}`);
    } catch (error) {
      setStatus(`FOLDER ERROR: ${String(error).toUpperCase()}`);
    } finally {
      input.value = "";
      setBusyKey(null);
    }
  };

  const panelStyle: CSSProperties = {
    width: "min(100%, 480px)",
    margin: "0 auto",
    background: "#0c140c",
    border: "1px solid #1a3a1a",
  };

  const sectionStyle: CSSProperties = {
    borderTop: "1px solid #1a3a1a",
    padding: 16,
  };

  const blockStyle: CSSProperties = {
    border: "1px solid #1a3a1a",
    padding: 16,
    background: "#081008",
  };

  const labelStyle: CSSProperties = {
    color: "#22ff44",
    fontSize: 12,
    letterSpacing: "0.08em",
  };

  const valueStyle: CSSProperties = {
    color: "#22ff44",
    fontSize: 14,
  };

  const actionButton = (color: string, busy = false): CSSProperties => ({
    flex: 1,
    height: 40,
    background: busy ? color : "#0a0f0a",
    color: busy ? "#0a0f0a" : color,
    border: `1px solid ${color}`,
    padding: "0 12px",
    textTransform: "uppercase",
    cursor: busy ? "wait" : "pointer",
  });

  const smallButtonStyle = (color: string): CSSProperties => ({
    minHeight: 32,
    padding: "0 10px",
    border: `1px solid ${color}`,
    background: "#0a0f0a",
    color,
    textTransform: "uppercase",
    cursor: "pointer",
  });

  return (
    <>
      <style>{globalStyles}</style>

      <main style={panelStyle} data-status={status}>
        <header
          style={{
            display: "grid",
            gridTemplateColumns: "32px 1fr auto",
            alignItems: "center",
            gap: 12,
            padding: 16,
            borderBottom: "1px solid #1a3a1a",
          }}
        >
          <div
            style={{
              width: 32,
              height: 32,
              border: "1px solid #22ff44",
              display: "grid",
              placeItems: "center",
              color: "#22ff44",
            }}
          >
            SW
          </div>
          <div style={{ color: "#c8ffcc", fontSize: 16, letterSpacing: "0.08em" }}>SLAPWINDOWS V1.0</div>
          <div style={{ display: "flex", gap: 8 }}>
            <button
              className="terminal-button"
              onClick={() => setShowGuide((current) => !current)}
              style={{
                border: "1px solid #1a3a1a",
                background: showGuide ? "#22ff44" : "#0a0f0a",
                color: showGuide ? "#0a0f0a" : "#22ff44",
                padding: "6px 8px",
                fontSize: 12,
                cursor: "pointer",
              }}
            >
              GUIDE
            </button>
            <div
              style={{
                border: `1px solid ${activeStatusColor}`,
                color: activeStatusColor,
                padding: "6px 8px",
                fontSize: 12,
              }}
            >
              {activeStatus}
            </div>
          </div>
        </header>

        <section style={sectionStyle}>
          <div style={blockStyle}>
            <div style={{ textAlign: "center", color: "#22ff44", fontSize: 48, lineHeight: 1 }}>{config.slap_count}</div>
            <div style={{ textAlign: "center", color: "#5a8a5a", fontSize: 12, marginTop: 8 }}>TOTAL SLAPS</div>
          </div>
        </section>

        <section style={sectionStyle}>
          <div style={{ display: "grid", gap: 14 }}>
            <div style={{ display: "grid", gap: 8 }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span style={labelStyle}>[ SENSITIVITY ]</span>
                <span style={valueStyle}>{formatValue(sensitivity)}</span>
              </div>
              <input
                type="range"
                min={0.02}
                max={0.5}
                step={0.01}
                value={sensitivity}
                style={sliderFill(sensitivity, 0.02, 0.5)}
                onChange={(event) => setSensitivity(Number(event.currentTarget.value))}
              />
            </div>

            <div style={{ display: "grid", gap: 8 }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span style={labelStyle}>[ SLAP FILTER ]</span>
                <span style={valueStyle}>{formatValue(slapFilter)}</span>
              </div>
              <input
                type="range"
                min={1}
                max={4}
                step={0.05}
                value={slapFilter}
                style={sliderFill(slapFilter, 1, 4)}
                onChange={(event) => setSlapFilter(Number(event.currentTarget.value))}
              />
            </div>

            <div style={{ display: "grid", gap: 8 }}>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span style={labelStyle}>[ COOLDOWN ]</span>
                <span style={valueStyle}>{formatValue(cooldown)}</span>
              </div>
              <input
                type="range"
                min={0.2}
                max={3}
                step={0.1}
                value={cooldown}
                style={sliderFill(cooldown, 0.2, 3)}
                onChange={(event) => setCooldown(Number(event.currentTarget.value))}
              />
            </div>
          </div>
        </section>

        <section style={sectionStyle}>
          <div style={{ ...labelStyle, marginBottom: 10 }}>[ SOUND PACK ]</div>
          <div style={{ display: "grid", gridTemplateColumns: "48px 1fr 48px", gap: 8 }}>
            <button
              className="pack-arrow"
              onClick={() => void cyclePack(-1)}
              disabled={busyKey === "pack" || visiblePacks.length === 0}
              style={{
                border: "1px solid #1a3a1a",
                background: "#0a0f0a",
                color: "#22ff44",
                height: 40,
                cursor: "pointer",
              }}
            >
              {"<"}
            </button>
            <div
              style={{
                border: `1px solid ${visiblePacks.length === 0 ? "#1a3a1a" : "#22ff44"}`,
                background: "#0a0f0a",
                color: visiblePacks.length === 0 ? "#5a8a5a" : "#c8ffcc",
                height: 40,
                display: "grid",
                placeItems: "center",
              }}
            >
              {visiblePacks.length === 0 ? "[ NO PACKS ]" : `[ ${displayPackName(config.settings.active_pack)} ]`}
            </div>
            <button
              className="pack-arrow"
              onClick={() => void cyclePack(1)}
              disabled={busyKey === "pack" || visiblePacks.length === 0}
              style={{
                border: "1px solid #1a3a1a",
                background: "#0a0f0a",
                color: "#22ff44",
                height: 40,
                cursor: "pointer",
              }}
            >
              {">"}
            </button>
          </div>
        </section>

        <section style={sectionStyle}>
          <div style={{ ...labelStyle, marginBottom: 10 }}>[ PLAYBACK MODE ]</div>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: 8 }}>
            {(["cycle", "random", "single"] as const).map((mode) => (
              <button
                key={mode}
                className={`terminal-button mode-button ${config.settings.playback_mode === mode ? "active" : ""}`}
                onClick={() =>
                  void updatePlaybackSettings(
                    {
                      playback_mode: mode,
                      selected_sound: mode === "single" ? config.settings.selected_sound ?? config.active_pack_sounds[0] ?? null : config.settings.selected_sound,
                    },
                    "playback-mode",
                    `MODE SET: ${mode.toUpperCase()}`,
                  )
                }
                disabled={busyKey === "playback-mode"}
                style={smallButtonStyle(config.settings.playback_mode === mode ? "#22ff44" : "#1a3a1a")}
              >
                [{mode.toUpperCase()}]
              </button>
            ))}
          </div>

          {config.settings.playback_mode === "single" ? (
            <div style={{ marginTop: 14, display: "grid", gap: 8 }}>
              <div style={labelStyle}>[ SINGLE SOUND ]</div>
              {config.active_pack_sounds.length === 0 ? (
                <div style={{ ...blockStyle, color: "#5a8a5a", fontSize: 12 }}>NO SOUND FILES FOUND</div>
              ) : (
                <div style={{ display: "grid", gap: 6, maxHeight: 160, overflowY: "auto" }}>
                  {config.active_pack_sounds.map((sound) => (
                    <button
                      key={sound}
                      className={`terminal-button sound-button ${config.settings.selected_sound === sound ? "active" : ""}`}
                      onClick={() =>
                        void updatePlaybackSettings(
                          { playback_mode: "single", selected_sound: sound },
                          "single-sound",
                          `SINGLE SOUND: ${sound.toUpperCase()}`,
                        )
                      }
                      disabled={busyKey === "single-sound"}
                      style={{
                        ...smallButtonStyle(config.settings.selected_sound === sound ? "#22ff44" : "#1a3a1a"),
                        textAlign: "left",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                      title={sound}
                    >
                      [{sound.toUpperCase()}]
                    </button>
                  ))}
                </div>
              )}
            </div>
          ) : null}
        </section>

        <section style={sectionStyle}>
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 8 }}>
            <button className="terminal-button" onClick={() => void runTestSound()} disabled={busyKey === "test"} style={actionButton("#22ff44", busyKey === "test")}>
              [ TEST SOUND ]
            </button>
            <button className="terminal-button danger-button" onClick={() => void resetSlapCount()} disabled={busyKey === "reset"} style={actionButton("#ff4444", busyKey === "reset")}>
              [ RESET COUNT ]
            </button>
          </div>
        </section>

        <section style={sectionStyle}>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              gap: 12,
              border: "1px solid #1a3a1a",
              padding: 12,
              background: "#0a0f0a",
            }}
          >
            <div style={{ color: "#c8ffcc", fontSize: 14 }}>AUTO-LAUNCH</div>
            <button
              className="toggle-button"
              onClick={() => void toggleAutolaunch()}
              disabled={busyKey === "autolaunch"}
              style={{
                minWidth: 64,
                height: 32,
                border: `1px solid ${config.autolaunch_enabled ? "#22ff44" : "#1a3a1a"}`,
                background: "#0a0f0a",
                color: config.autolaunch_enabled ? "#22ff44" : "#5a8a5a",
                cursor: "pointer",
              }}
            >
              {config.autolaunch_enabled ? "[ON ]" : "[OFF]"}
            </button>
          </div>
        </section>

        <section style={sectionStyle}>
          <div style={{ ...labelStyle, marginBottom: 10 }}>[ BONUS ZONE ]</div>

          <input
            ref={fileInputRef}
            type="file"
            accept=".wav,.mp3,.ogg,audio/wav,audio/mpeg,audio/ogg"
            multiple
            onChange={handleCustomSoundImport}
            style={{ display: "none" }}
          />

          <input
            ref={folderInputRef}
            type="file"
            multiple
            onChange={handleFolderImport}
            style={{ display: "none" }}
            {...({ webkitdirectory: "", directory: "" } as Record<string, string>)}
          />

          <div style={{ display: "grid", gap: 8 }}>
            <button
              className="terminal-button"
              onClick={openFilePicker}
              disabled={busyKey === "bonus-files"}
              style={actionButton("#22ff44", busyKey === "bonus-files")}
            >
              [ ADD FILE TO CURRENT PACK ]
            </button>
            <button
              className="terminal-button"
              onClick={openFolderPicker}
              disabled={busyKey === "bonus-folder"}
              style={actionButton("#22ff44", busyKey === "bonus-folder")}
            >
              [ UPLOAD A NEW PACK ]
            </button>
          </div>
        </section>

        <section style={sectionStyle}>
          <div style={{ ...labelStyle, marginBottom: 10 }}>[ PACK FILES ]</div>
          {config.active_pack_sounds.length === 0 ? (
            <div style={{ ...blockStyle, color: "#5a8a5a", fontSize: 12 }}>NO FILES IN THIS PACK</div>
          ) : (
            <div style={{ display: "grid", gap: 6, maxHeight: 180, overflowY: "auto" }}>
              {config.active_pack_sounds.map((sound) => (
                <div
                  key={sound}
                  style={{
                    border: "1px solid #1a3a1a",
                    background: "#0a0f0a",
                    padding: 8,
                    display: "grid",
                    gridTemplateColumns: "1fr auto",
                    gap: 8,
                    alignItems: "center",
                  }}
                >
                  <div style={{ color: "#c8ffcc", fontSize: 12, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }} title={sound}>
                    {sound.toUpperCase()}
                  </div>
                  <button
                    className="terminal-button danger-button"
                    onClick={() => void removeSound(sound)}
                    disabled={busyKey === `remove-${sound}`}
                    style={smallButtonStyle("#ff4444")}
                  >
                    [DEL]
                  </button>
                </div>
              ))}
            </div>
          )}
        </section>

        <footer style={{ borderTop: "1px solid #1a3a1a", padding: 16, color: "#5a8a5a", fontSize: 12, display: "grid", gap: 6 }}>
          <div style={{ display: "flex", justifyContent: "space-between", gap: 8, alignItems: "center" }}>
            <div>{`// slap count persists across sessions`}</div>
            <button
              className="terminal-button"
              onClick={() => {
                setQrMissing(false);
                setMemeMissing(false);
                setShowDonate(true);
              }}
              style={{
                border: "1px solid #1a3a1a",
                background: "#0a0f0a",
                color: "#5a8a5a",
                padding: "6px 10px",
                fontSize: 12,
                cursor: "pointer",
              }}
            >
              DON'T CLICK ME
            </button>
          </div>
          <div>{`// mic: ${micName}`}</div>
        </footer>
      </main>

      {showGuide ? (
        <div className="guide-overlay" onClick={() => setShowGuide(false)}>
          <div className="guide-modal" onClick={(event) => event.stopPropagation()}>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                gap: 12,
                padding: 16,
                borderBottom: "1px solid #1a3a1a",
              }}
            >
              <div style={{ ...labelStyle, fontSize: 14 }}>[ HOW IT WORKS ]</div>
              <button
                className="terminal-button"
                onClick={() => setShowGuide(false)}
                style={{
                  border: "1px solid #1a3a1a",
                  background: "#0a0f0a",
                  color: "#22ff44",
                  padding: "6px 10px",
                  cursor: "pointer",
                }}
              >
                CLOSE
              </button>
            </div>

            <div style={{ display: "grid", gap: 12, padding: 16, color: "#5a8a5a", fontSize: 12 }}>
              <div style={{ ...blockStyle, display: "grid", gap: 8 }}>
                <div style={labelStyle}>[ DETECTION CONTROLS ]</div>
                <div>// SENSITIVITY: LOWER VALUES TRIGGER MORE EASILY. HIGHER VALUES NEED HARDER HITS.</div>
                <div>// SLAP FILTER: HIGHER VALUES REJECT SHARPER NOISES AND FAVOR DEEPER THUDS.</div>
                <div>// COOLDOWN: ADDS A WAIT AFTER EACH HIT SO ONE IMPACT DOES NOT TRIGGER MULTIPLE TIMES.</div>
              </div>

              <div style={{ ...blockStyle, display: "grid", gap: 8 }}>
                <div style={labelStyle}>[ SOUND PACKS ]</div>
                <div>// UPLOAD A NEW PACK TO IMPORT A FOLDER OF AUDIO FILES AS A CUSTOM PACK.</div>
                <div>// ADD FILE TO CURRENT PACK TO APPEND MORE SOUNDS INTO THE SELECTED CUSTOM PACK.</div>
                <div>// THE DEFAULT CUSTOM PACK IS SHOWN AS DEFAULT.</div>
              </div>

              <div style={{ ...blockStyle, display: "grid", gap: 8 }}>
                <div style={labelStyle}>[ PLAYBACK ]</div>
                <div>// CYCLE PLAYS FILES ONE BY ONE.</div>
                <div>// RANDOM PICKS A DIFFERENT SOUND AUTOMATICALLY.</div>
                <div>// SINGLE LETS YOU CHOOSE ONE EXACT FILE.</div>
                <div>// USE [DEL] IN PACK FILES TO REMOVE CUSTOM AUDIO.</div>
              </div>
            </div>
          </div>
        </div>
      ) : null}

      {showDonate ? (
        <div className="guide-overlay" onClick={() => setShowDonate(false)}>
          <div className="donation-modal" onClick={(event) => event.stopPropagation()}>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                gap: 12,
                padding: 16,
                borderBottom: "1px solid #1a3a1a",
              }}
            >
              <div style={{ ...labelStyle, fontSize: 14 }}>[ YOU CLICKED IT ]</div>
              <button
                className="terminal-button"
                onClick={() => setShowDonate(false)}
                style={{
                  border: "1px solid #1a3a1a",
                  background: "#0a0f0a",
                  color: "#22ff44",
                  padding: "6px 10px",
                  cursor: "pointer",
                }}
              >
                CLOSE
              </button>
            </div>

            <div style={{ display: "grid", gap: 12, padding: 16 }}>
              <div style={{ ...blockStyle, display: "grid", gap: 8, textAlign: "center" }}>
                {/* <div style={labelStyle}>[ MEME FIRST ]</div> */}
                {!memeMissing ? (
                  <img
                    src={DONATION_MEME_SRC}
                    alt="Donation meme"
                    onError={() => setMemeMissing(true)}
                    style={{
                      width: "99%",
                      maxHeight: 220,
                      objectFit: "cover",
                      border: "1px solid #1a3a1a",
                      background: "#0a0f0a",
                    }}
                  />
                ) : (
                  <>
                    <div style={{ color: "#c8ffcc", fontSize: 18, lineHeight: 1.4 }}>YOU PRESSED</div>
                    <div style={{ color: "#22ff44", fontSize: 28, lineHeight: 1.2 }}>DON'T CLICK ME</div>
                    <div style={{ color: "#5a8a5a", fontSize: 12 }}>ADD /PUBLIC/POOR.GIF TO SHOW THE MEME HERE</div>
                  </>
                )}
              </div>

              <div style={{ ...blockStyle, display: "grid", gap: 10, justifyItems: "center" }}>
                <div style={labelStyle}>[ Bhandara Kra Do Babuji ]</div>
                {!qrMissing ? (
                  <img
                    src={DONATION_UPI_QR_SRC}
                    alt="UPI QR"
                    onError={() => setQrMissing(true)}
                    style={{
                      width: 220,
                      height: 220,
                      objectFit: "cover",
                      border: "1px solid #1a3a1a",
                      background: "#0a0f0a",
                      imageRendering: "pixelated",
                    }}
                  />
                ) : (
                  <div
                    style={{
                      width: 220,
                      height: 220,
                      border: "1px solid #1a3a1a",
                      background: "#0a0f0a",
                      display: "grid",
                      placeItems: "center",
                      color: "#5a8a5a",
                      fontSize: 12,
                      textAlign: "center",
                      padding: 16,
                    }}
                  >
                  </div>
                )}
              </div>
              <a
                className="terminal-button"
                href={REPO_URL}
                target="_blank"
                rel="noreferrer"
                style={{
                  ...actionButton("#5a8a5a"),
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: 10,
                  textDecoration: "none",
                }}
              >
                <FaGithub size={16} aria-hidden="true" style={{ flexShrink: 0 }} />
                <span>[ GIVE A STAR ]</span>
              </a>
            </div>
          </div>
        </div>
      ) : null}
    </>
  );
}

export default App;
