import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { prefersCalm } from "./sound";

export interface FxHandle {
  sparks: (x: number, y: number, color: string, count: number) => void;
}

interface Spark {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  color: string;
  r: number;
  square: boolean;
  rot: number;
}

/**
 * 画面最上面那层：飘着的星尘、点击时的火花，以及跟着鼠标的轻微视差
 * （写进舞台的 --mx / --my）。关了动效的人这一层什么都不画。
 */
export const Fx = forwardRef<FxHandle, { stage: () => HTMLElement | null }>(
  function Fx({ stage }, ref) {
    const canvas = useRef<HTMLCanvasElement>(null);
    const sparks = useRef<Spark[]>([]);
    const stageRef = useRef(stage);
    stageRef.current = stage;

    useImperativeHandle(ref, () => ({
      sparks(x, y, color, count) {
        if (prefersCalm()) return;
        for (let i = 0; i < count; i++) {
          const angle = Math.random() * 6.283;
          const speed = 80 + Math.random() * 380;
          sparks.current.push({
            x,
            y,
            vx: Math.cos(angle) * speed,
            vy: Math.sin(angle) * speed - 60,
            life: 1,
            color,
            r: 1.2 + Math.random() * 3,
            square: Math.random() < 0.5,
            rot: Math.random() * 6,
          });
        }
      },
    }));

    useEffect(() => {
      const node = canvas.current;
      const context = node?.getContext("2d");
      if (!node || !context || prefersCalm()) return;
      let width = 0;
      let height = 0;
      const size = () => {
        const ratio = Math.min(window.devicePixelRatio || 1, 2);
        width = window.innerWidth;
        height = window.innerHeight;
        node.width = width * ratio;
        node.height = height * ratio;
        context.setTransform(ratio, 0, 0, ratio, 0, 0);
      };
      size();
      let targetX = 0;
      let targetY = 0;
      let x = 0;
      let y = 0;
      const move = (event: PointerEvent) => {
        targetX = (event.clientX / window.innerWidth) * 2 - 1;
        targetY = (event.clientY / window.innerHeight) * 2 - 1;
      };
      window.addEventListener("resize", size);
      window.addEventListener("pointermove", move);
      const dust = Array.from({ length: 34 }, () => ({
        x: Math.random(),
        y: Math.random(),
        r: 0.8 + Math.random() * Math.random() * 3,
        v: 0.008 + Math.random() * 0.02,
        p: Math.random() * 6.28,
        s: 0.6 + Math.random() * 1.2,
      }));
      let last = performance.now();
      let frame = 0;
      const draw = (now: number) => {
        const dt = Math.min(0.05, (now - last) / 1000);
        last = now;
        x += (targetX - x) * 0.04;
        y += (targetY - y) * 0.04;
        const root = stageRef.current();
        root?.style.setProperty("--mx", x.toFixed(3));
        root?.style.setProperty("--my", y.toFixed(3));
        context.clearRect(0, 0, width, height);
        if (root?.getAttribute("data-phase") === "home")
          for (const mote of dust) {
            mote.y -= mote.v * dt;
            mote.p += dt * mote.s;
            if (mote.y < -0.05) {
              mote.y = 1.05;
              mote.x = Math.random();
            }
            const px = (mote.x + Math.sin(mote.p) * 0.006) * width;
            const py = mote.y * height;
            const alpha = 0.2 + 0.5 * Math.sin(mote.p * 1.7) ** 2;
            const radius = mote.r * 2.2;
            const glow = context.createRadialGradient(
              px,
              py,
              0,
              px,
              py,
              radius,
            );
            glow.addColorStop(0, `rgba(214,238,255,${alpha})`);
            glow.addColorStop(1, "rgba(214,238,255,0)");
            context.fillStyle = glow;
            context.beginPath();
            context.arc(px, py, radius, 0, 6.283);
            context.fill();
          }
        context.globalCompositeOperation = "lighter";
        sparks.current = sparks.current.filter((spark) => spark.life > 0);
        for (const spark of sparks.current) {
          spark.life -= dt * 1.1;
          spark.x += spark.vx * dt;
          spark.y += spark.vy * dt;
          spark.vy += 300 * dt;
          spark.vx *= 0.985;
          spark.rot += dt * 6;
          context.globalAlpha = Math.max(0, spark.life);
          context.fillStyle = spark.color;
          if (spark.square) {
            context.save();
            context.translate(spark.x, spark.y);
            context.rotate(spark.rot);
            context.fillRect(
              -spark.r * 1.6,
              -spark.r * 0.5,
              spark.r * 3.2,
              spark.r,
            );
            context.restore();
          } else {
            context.beginPath();
            context.arc(
              spark.x,
              spark.y,
              spark.r * (0.4 + spark.life),
              0,
              6.283,
            );
            context.fill();
          }
        }
        context.globalAlpha = 1;
        context.globalCompositeOperation = "source-over";
        frame = requestAnimationFrame(draw);
      };
      frame = requestAnimationFrame(draw);
      return () => {
        cancelAnimationFrame(frame);
        window.removeEventListener("resize", size);
        window.removeEventListener("pointermove", move);
      };
    }, []);

    return <canvas id="fx" ref={canvas} aria-hidden="true" />;
  },
);
