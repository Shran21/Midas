import { useCallback, useEffect, useId, useRef, useState } from "react";
import { api, type DeviceSummary } from "../api";
import type { Language } from "../i18n";
import {
  Gestures,
  HOLD,
  IDENTITY,
  type Output,
  type View,
  zoomBy,
} from "./gestures";
import type { RemoteRequest } from "./model";
import { TransferPanel } from "./TransferPanel";
import { viewerCopy } from "./viewerCopy";
import {
  bitrate,
  COMMON_KEYS,
  combination,
  keyCode,
  modifiers,
  readIceServers,
  readSasResult,
  readStats,
  type Stats,
  SYSTEM_KEYS,
  TEXT_LIMIT,
  textMessages,
  videoPoint,
} from "./viewerModel";

type Props = {
  tenant: string;
  device: DeviceSummary;
  request: RemoteRequest;
  csrf: string;
  language: Language;
  fail: (error: unknown) => boolean;
};

type Phase = "connecting" | "live" | "failed" | "closed";
/** A report older than this no longer describes the connection. */
const STALE = 3000;
const GATHERING = 3000;

async function gathered(connection: RTCPeerConnection) {
  if (connection.iceGatheringState === "complete") return;
  await new Promise<void>((resolve) => {
    const done = () => {
      if (connection.iceGatheringState === "complete") {
        connection.removeEventListener("icegatheringstatechange", done);
        resolve();
      }
    };
    connection.addEventListener("icegatheringstatechange", done);
    setTimeout(resolve, GATHERING);
  });
}

/** The live picture of an active request; pointer and keys go to the agent while it has focus. */
export function Viewer({
  tenant,
  device,
  request,
  csrf,
  language,
  fail,
}: Props) {
  const c = viewerCopy[language];
  const id = useId();
  const video = useRef<HTMLVideoElement>(null);
  const surface = useRef<HTMLDivElement>(null);
  const channel = useRef<RTCDataChannel | null>(null);
  const [phase, setPhase] = useState<Phase>("connecting");
  const [stats, setStats] = useState<{ value: Stats; at: number } | null>(null);
  const [now, setNow] = useState(() => performance.now());
  const [profile, setProfile] = useState<"text" | "motion">("text");
  const [sas, setSas] = useState<"sent" | "unavailable" | null>(null);
  const [typed, setTyped] = useState("");
  const [view, setView] = useState<View>(IDENTITY);
  const surfaceSize = useCallback(() => {
    const box = surface.current?.getBoundingClientRect();
    return { width: box?.width ?? 0, height: box?.height ?? 0 };
  }, []);
  const gestures = useRef<Gestures | null>(null);
  gestures.current ??= new Gestures(surfaceSize);
  const hold = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [transferChannel, setTransferChannel] = useState<RTCDataChannel | null>(
    null,
  );
  const transfers =
    request.transfer.clipboard ||
    request.transfer.upload ||
    request.transfer.download;
  const latest = useRef({ fail, csrf });
  latest.current = { fail, csrf };

  useEffect(() => {
    let closed = false;
    let connection: RTCPeerConnection | null = null;
    (async () => {
      try {
        // The relay is for when no direct path works; without one the browser tries direct only.
        const iceServers = await api
          .remoteIce(tenant, device.id, request.id)
          .then(readIceServers, () => []);
        if (closed) return;
        const peer = new RTCPeerConnection({ iceServers });
        connection = peer;
        peer.addTransceiver("video", { direction: "recvonly" });
        const control = peer.createDataChannel("control");
        channel.current = control;
        control.onmessage = (event) => {
          const text = String(event.data);
          const value = readStats(text);
          if (value) setStats({ value, at: performance.now() });
          const sas = readSasResult(text);
          if (sas) setSas(sas);
        };
        // Bulk data has its own channel, so it never holds up input.
        if (transfers) setTransferChannel(peer.createDataChannel("transfer"));
        peer.ontrack = (event) => {
          if (video.current)
            video.current.srcObject =
              event.streams[0] ?? new MediaStream([event.track]);
        };
        peer.onconnectionstatechange = () => {
          if (closed) return;
          const state = peer.connectionState;
          if (state === "connected") setPhase("live");
          if (state === "failed") setPhase("failed");
          if (state === "closed" || state === "disconnected")
            setPhase("closed");
        };
        await peer.setLocalDescription(await peer.createOffer());
        await gathered(peer);
        const sdp = peer.localDescription?.sdp;
        if (!sdp || closed) return;
        const response = await api.remoteMedia(
          tenant,
          device.id,
          request.id,
          sdp,
          latest.current.csrf,
        );
        if (closed) return;
        if (
          typeof response !== "object" ||
          response === null ||
          typeof (response as { sdp?: unknown }).sdp !== "string"
        )
          throw new Error("invalid answer");
        await peer.setRemoteDescription({
          type: "answer",
          sdp: (response as { sdp: string }).sdp,
        });
      } catch (error) {
        if (!closed && !latest.current.fail(error)) setPhase("failed");
      }
    })();
    const tick = setInterval(() => setNow(performance.now()), 1000);
    return () => {
      closed = true;
      clearInterval(tick);
      channel.current = null;
      setTransferChannel(null);
      connection?.close();
    };
  }, [tenant, device.id, request.id, transfers]);

  const send = useCallback((message: object) => {
    const control = channel.current;
    if (control?.readyState === "open") control.send(JSON.stringify(message));
  }, []);

  const press = (
    clientX: number,
    clientY: number,
    buttons: number,
    held = 0,
  ) => {
    const element = video.current;
    if (!element) return;
    const point = videoPoint(
      clientX,
      clientY,
      element.getBoundingClientRect(),
      {
        width: element.videoWidth,
        height: element.videoHeight,
      },
    );
    if (point) send({ type: "pointer", ...point, buttons, modifiers: held });
  };

  const pointer = (event: React.PointerEvent<HTMLDivElement>) =>
    press(event.clientX, event.clientY, event.buttons & 31, modifiers(event));

  // What a touch gesture produced: presses for the remote computer, or a new zoom.
  const apply = (out: Output[]) => {
    const box = surface.current?.getBoundingClientRect();
    if (!box) return;
    for (const item of out) {
      if ("view" in item) setView(item.view);
      else
        press(
          box.left + item.press.x,
          box.top + item.press.y,
          item.press.buttons,
        );
    }
  };

  const touch = (
    event: React.PointerEvent<HTMLDivElement>,
    phase: "down" | "move" | "up" | "cancel",
  ) => {
    const box = surface.current?.getBoundingClientRect();
    const model = gestures.current;
    if (!box || !model) return;
    const at = performance.now();
    const point = { x: event.clientX - box.left, y: event.clientY - box.top };
    if (phase === "down") {
      apply(model.down(event.pointerId, point, at));
      clearTimeout(hold.current);
      hold.current = setTimeout(
        () => apply(model.tick(performance.now())),
        HOLD + 20,
      );
    } else if (phase === "move") apply(model.move(event.pointerId, point));
    else if (phase === "up") apply(model.up(event.pointerId, point, at));
    else apply(model.cancel());
  };

  const zoom = (factor: number | null) => {
    const model = gestures.current;
    if (!model) return;
    model.setView(
      factor === null ? IDENTITY : zoomBy(model.view, factor, surfaceSize()),
    );
    setView(model.view);
  };

  useEffect(() => () => clearTimeout(hold.current), []);

  useEffect(() => {
    const element = surface.current;
    if (!element) return;
    // The page must not scroll while the wheel scrolls the remote desktop.
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const clamp = (value: number) =>
        Math.max(-1200, Math.min(1200, Math.round(value)));
      send({ type: "wheel", dx: clamp(event.deltaX), dy: clamp(event.deltaY) });
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [send]);

  const key = (event: React.KeyboardEvent<HTMLDivElement>, down: boolean) => {
    const code = keyCode(event.code);
    if (!code) return;
    event.preventDefault();
    send({ type: "key", code, down });
  };

  const choose = (next: "text" | "motion") => {
    setProfile(next);
    send({ type: "profile", profile: next });
  };

  const fresh = stats && now - stats.at < STALE ? stats.value : null;
  const state = phase === "live" ? (fresh ? "live" : "unmeasured") : phase;
  return (
    <section className="remote-viewer" aria-labelledby={`${id}-title`}>
      <h3 id={`${id}-title`}>{c.title}</h3>
      <div
        ref={surface}
        className="remote-viewer-surface"
        role="application"
        aria-label={c.surface}
        aria-describedby={`${id}-hint`}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: the remote desktop takes the keyboard while focused
        tabIndex={0}
        onPointerMove={(event) =>
          event.pointerType === "touch" ? touch(event, "move") : pointer(event)
        }
        onPointerDown={(event) => {
          surface.current?.focus();
          event.currentTarget.setPointerCapture(event.pointerId);
          if (event.pointerType === "touch") touch(event, "down");
          else pointer(event);
        }}
        onPointerUp={(event) =>
          event.pointerType === "touch" ? touch(event, "up") : pointer(event)
        }
        onPointerCancel={(event) => {
          if (event.pointerType === "touch") touch(event, "cancel");
        }}
        onContextMenu={(event) => event.preventDefault()}
        onKeyDown={(event) => key(event, true)}
        onKeyUp={(event) => key(event, false)}
      >
        <video
          ref={video}
          autoPlay
          muted
          playsInline
          style={{
            transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})`,
          }}
        />
      </div>
      <p id={`${id}-hint`} className="muted">
        {c.hint} {c.touch}
      </p>
      <fieldset className="remote-viewer-zoom">
        <legend>{c.zoom}</legend>
        <button
          type="button"
          className="button"
          aria-label={c.zoomOut}
          disabled={view.scale <= 1}
          onClick={() => zoom(1 / 1.5)}
        >
          −
        </button>
        <span aria-live="polite">
          {c.zoomLevel(Math.round(view.scale * 100))}
        </span>
        <button
          type="button"
          className="button"
          aria-label={c.zoomIn}
          disabled={view.scale >= 4}
          onClick={() => zoom(1.5)}
        >
          +
        </button>
        <button
          type="button"
          className="button"
          disabled={view.scale === 1}
          onClick={() => zoom(null)}
        >
          {c.zoomReset}
        </button>
      </fieldset>
      <div className="remote-viewer-bar">
        <p role="status" aria-live="polite">
          {c[state]}
        </p>
        <p
          className="remote-viewer-desktop"
          role="status"
          aria-live="polite"
          hidden={!fresh || fresh.desktop === "user"}
        >
          {fresh?.desktop === "secure"
            ? c.secureDesktop
            : fresh?.desktop === "elevated"
              ? c.elevatedWindow
              : ""}
        </p>
        <p
          className="remote-viewer-degraded"
          role="status"
          aria-live="polite"
          hidden={!fresh?.degraded}
        >
          {fresh?.degraded ? c.degraded : ""}
        </p>
        {fresh && (
          <p className="muted">
            {c.quality(
              fresh.auto,
              `${fresh.width}×${fresh.height}`,
              bitrate(fresh.sentBitrate, language),
              fresh.sentFps,
            )}
          </p>
        )}
        {fresh && (
          <p className="muted">
            {c.network(fresh.rttMs, fresh.loss)}
            {fresh.path && ` · ${c[fresh.path]}`}
          </p>
        )}
        <fieldset className="remote-viewer-profile">
          <legend>{c.profile}</legend>
          {(["text", "motion"] as const).map((value) => (
            <button
              key={value}
              type="button"
              className="button"
              aria-pressed={profile === value}
              onClick={() => choose(value)}
            >
              {c[value]}
            </button>
          ))}
        </fieldset>
        <fieldset className="remote-viewer-keys">
          <legend>{c.common}</legend>
          {(Object.keys(COMMON_KEYS) as (keyof typeof COMMON_KEYS)[]).map(
            (name) => (
              <button
                key={name}
                type="button"
                className="button"
                disabled={phase !== "live"}
                onClick={() => {
                  for (const message of combination(COMMON_KEYS[name]))
                    send(message);
                }}
              >
                {c[name]}
              </button>
            ),
          )}
        </fieldset>
        <fieldset className="remote-viewer-keys">
          <legend>{c.keys}</legend>
          {(Object.keys(SYSTEM_KEYS) as (keyof typeof SYSTEM_KEYS)[]).map(
            (name) => (
              <button
                key={name}
                type="button"
                className="button"
                disabled={phase !== "live"}
                onClick={() => {
                  for (const message of combination(SYSTEM_KEYS[name]))
                    send(message);
                }}
              >
                {c[name]}
              </button>
            ),
          )}
          {request.uac && (
            <button
              type="button"
              className="button"
              disabled={phase !== "live"}
              onClick={() => {
                setSas(null);
                send({ type: "sas" });
              }}
            >
              {c.sas}
            </button>
          )}
          <p className="muted">{c.secureAttention}</p>
          <p role="status" aria-live="polite" hidden={!sas}>
            {sas === "sent"
              ? c.sasSent
              : sas === "unavailable"
                ? c.sasUnavailable
                : ""}
          </p>
        </fieldset>
        <form
          className="remote-viewer-text"
          onSubmit={(event) => {
            event.preventDefault();
            for (const message of textMessages(typed)) send(message);
            setTyped("");
          }}
        >
          <label htmlFor={`${id}-text`}>{c.typeText}</label>
          <textarea
            id={`${id}-text`}
            rows={2}
            value={typed}
            maxLength={TEXT_LIMIT}
            disabled={phase !== "live"}
            aria-describedby={`${id}-text-hint`}
            onChange={(event) => setTyped(event.target.value)}
          />
          <button
            type="submit"
            className="button"
            disabled={phase !== "live" || typed === ""}
          >
            {c.send}
          </button>
          <p id={`${id}-text-hint`} className="muted">
            {c.textHint}
          </p>
        </form>
        {transferChannel && (
          <TransferPanel
            channel={transferChannel}
            rights={request.transfer}
            language={language}
          />
        )}
      </div>
    </section>
  );
}
