#!/usr/bin/env node

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import puppeteer from 'puppeteer';

const MOTIFS = [
  { id: '01_iridescent_chrome_heart', label: 'Iridescent Chrome Heart' },
  { id: '02_cloud_gate', label: 'Cloud Gate' },
  { id: '03_error_window_drift', label: '90s Error Window Drift' },
  { id: '04_wireframe_star_trails', label: 'Wireframe Star Trails' },
  { id: '05_dolphin_prism', label: 'Dolphin Prism' },
  { id: '06_celestial_dither_sun', label: 'Celestial Dither Sun' },
  { id: '07_y2k_tech_ring', label: 'Y2K Tech Ring' },
  { id: '08_cdrom_rainbow', label: 'CD-ROM Rainbow' },
  { id: '09_falling_data_water', label: 'Falling Data Water' },
  { id: '10_butterfly_pulse', label: 'Butterfly Pulse' },
];

function parseArgs(argv) {
  const config = {
    width: 1080,
    height: 1080,
    fps: 60,
    duration: 5,
    outDir: path.resolve(process.cwd(), 'renders/dreamcore_y2k_pack'),
    motifs: [],
    keepFrames: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--width') config.width = Number(argv[++i]);
    else if (arg === '--height') config.height = Number(argv[++i]);
    else if (arg === '--fps') config.fps = Number(argv[++i]);
    else if (arg === '--duration') config.duration = Number(argv[++i]);
    else if (arg === '--out-dir') config.outDir = path.resolve(process.cwd(), argv[++i]);
    else if (arg === '--motif') config.motifs.push(argv[++i]);
    else if (arg === '--keep-frames') config.keepFrames = true;
    else if (arg === '--help' || arg === '-h') {
      console.log(`Usage: node render_dreamcore_pack.js [options]

Options:
  --width <n>       Frame width (default: 1080)
  --height <n>      Frame height (default: 1080)
  --fps <n>         Frames per second (default: 60)
  --duration <s>    Seconds per asset (default: 5)
  --out-dir <path>  Output directory (default: renders/dreamcore_y2k_pack)
  --motif <id>      Render one motif id (repeat for multiple)
  --keep-frames     Keep temporary PNG frames for debugging
  --help            Show this message

Motif IDs:
${MOTIFS.map((m) => `  ${m.id}`).join('\n')}`);
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!Number.isFinite(config.width) || config.width <= 0) throw new Error('Invalid --width');
  if (!Number.isFinite(config.height) || config.height <= 0) throw new Error('Invalid --height');
  if (!Number.isFinite(config.fps) || config.fps <= 0) throw new Error('Invalid --fps');
  if (!Number.isFinite(config.duration) || config.duration <= 0) throw new Error('Invalid --duration');

  return config;
}

function runCommand(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, {
    stdio: opts.capture ? ['ignore', 'pipe', 'pipe'] : 'inherit',
    encoding: 'utf8',
  });
  if (result.status !== 0) {
    const stderr = result.stderr ? `\n${result.stderr}` : '';
    throw new Error(`${cmd} failed with code ${result.status}${stderr}`);
  }
  return result;
}

function encodeProRes4444(tempDir, fps, outputPath) {
  const inputPattern = path.join(tempDir, 'frame_%04d.png');
  const args = [
    '-y',
    '-framerate',
    String(fps),
    '-i',
    inputPattern,
    '-c:v',
    'prores_ks',
    '-profile:v',
    '4',
    '-pix_fmt',
    'yuva444p12le',
    '-vendor',
    'apl0',
    '-metadata:s:v:0',
    'vendor_id=apl0',
    outputPath,
  ];
  runCommand('ffmpeg', args);
}

function probeVideo(outputPath) {
  const args = [
    '-v',
    'error',
    '-select_streams',
    'v:0',
    '-show_entries',
    'stream=codec_name,codec_tag_string,pix_fmt,width,height,r_frame_rate,duration',
    '-of',
    'default=noprint_wrappers=1:nokey=0',
    outputPath,
  ];
  const result = runCommand('ffprobe', args, { capture: true });
  const lines = result.stdout.trim().split('\n').filter(Boolean);
  const parsed = {};
  for (const line of lines) {
    const idx = line.indexOf('=');
    if (idx > 0) {
      parsed[line.slice(0, idx)] = line.slice(idx + 1);
    }
  }
  return parsed;
}

function buildRuntimeHtml() {
  return String.raw`<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8" />
  <style>
    html, body {
      margin: 0;
      padding: 0;
      background: transparent;
      overflow: hidden;
    }
    canvas {
      display: block;
      background: transparent;
    }
  </style>
</head>
<body>
  <canvas id="frame"></canvas>
  <script>
    const TAU = Math.PI * 2;
    const canvas = document.getElementById('frame');
    const ctx = canvas.getContext('2d', { alpha: true });

    let W = 1080;
    let H = 1080;

    function setSize(w, h) {
      if (W !== w || H !== h || canvas.width !== w || canvas.height !== h) {
        W = w;
        H = h;
        canvas.width = w;
        canvas.height = h;
      }
    }

    function clamp(v, a, b) {
      return Math.min(Math.max(v, a), b);
    }

    function fract(x) {
      return x - Math.floor(x);
    }

    function hash(n) {
      return fract(Math.sin(n * 12.9898) * 43758.5453123);
    }

    function hsla(h, s, l, a) {
      return 'hsla(' + h + ' ' + s + '% ' + l + '% / ' + a + ')';
    }

    function clearFrame() {
      ctx.clearRect(0, 0, W, H);
    }

    function drawDust(time, hueA, hueB, count, spread) {
      const cx = W * 0.5;
      const cy = H * 0.5;
      const radius = Math.min(W, H) * spread;
      for (let i = 0; i < count; i += 1) {
        const seed = i * 17.31;
        const ang = hash(seed) * TAU + time * (0.05 + hash(seed + 2.1) * 0.25);
        const dist = radius * Math.pow(hash(seed + 8.7), 0.65);
        const x = cx + Math.cos(ang) * dist;
        const y = cy + Math.sin(ang) * dist;
        const size = 1 + hash(seed + 4.2) * 2.8;
        const hue = hueA + (hueB - hueA) * hash(seed + 5.6);
        const alpha = 0.06 + hash(seed + 1.9) * 0.22;
        ctx.beginPath();
        ctx.fillStyle = hsla(hue, 95, 85, alpha);
        ctx.arc(x, y, size, 0, TAU);
        ctx.fill();
      }
    }

    function roundedRectPath(x, y, w, h, r) {
      const radius = Math.min(r, w * 0.5, h * 0.5);
      ctx.beginPath();
      ctx.moveTo(x + radius, y);
      ctx.lineTo(x + w - radius, y);
      ctx.quadraticCurveTo(x + w, y, x + w, y + radius);
      ctx.lineTo(x + w, y + h - radius);
      ctx.quadraticCurveTo(x + w, y + h, x + w - radius, y + h);
      ctx.lineTo(x + radius, y + h);
      ctx.quadraticCurveTo(x, y + h, x, y + h - radius);
      ctx.lineTo(x, y + radius);
      ctx.quadraticCurveTo(x, y, x + radius, y);
      ctx.closePath();
    }

    function starPoints(cx, cy, outerR, innerR, points, rotation) {
      const out = [];
      for (let i = 0; i < points * 2; i += 1) {
        const rr = i % 2 === 0 ? outerR : innerR;
        const a = rotation + (i * Math.PI) / points;
        out.push({ x: cx + Math.cos(a) * rr, y: cy + Math.sin(a) * rr });
      }
      return out;
    }

    function strokePath(points, color, width, alpha, blur) {
      ctx.save();
      ctx.strokeStyle = color;
      ctx.globalAlpha = alpha;
      ctx.lineWidth = width;
      ctx.shadowColor = color;
      ctx.shadowBlur = blur;
      ctx.beginPath();
      ctx.moveTo(points[0].x, points[0].y);
      for (let i = 1; i < points.length; i += 1) {
        ctx.lineTo(points[i].x, points[i].y);
      }
      ctx.closePath();
      ctx.stroke();
      ctx.restore();
    }

    function heartPath(scale) {
      const steps = 220;
      ctx.beginPath();
      for (let i = 0; i <= steps; i += 1) {
        const a = (i / steps) * TAU;
        const x = 16 * Math.pow(Math.sin(a), 3) * scale;
        const y = -(13 * Math.cos(a) - 5 * Math.cos(2 * a) - 2 * Math.cos(3 * a) - Math.cos(4 * a)) * scale;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.closePath();
    }

    function drawIridescentChromeHeart(time) {
      const cx = W * 0.5 + Math.sin(time * 1.1) * W * 0.015;
      const cy = H * 0.53 + Math.sin(time * 2.3) * H * 0.012;
      const scale = (Math.min(W, H) / 1080) * 13.2;
      const pulse = 1 + Math.sin(time * 2.0) * 0.05;
      const hueBase = (time * 48) % 360;

      ctx.save();
      ctx.translate(cx, cy);
      ctx.scale(pulse, pulse);

      heartPath(scale);
      const grad = ctx.createLinearGradient(-250, -250, 250, 250);
      grad.addColorStop(0, hsla((hueBase + 186) % 360, 98, 79, 0.95));
      grad.addColorStop(0.38, hsla((hueBase + 320) % 360, 96, 73, 0.92));
      grad.addColorStop(0.7, hsla((hueBase + 45) % 360, 98, 83, 0.95));
      grad.addColorStop(1, hsla((hueBase + 220) % 360, 96, 76, 0.95));
      ctx.fillStyle = grad;
      ctx.shadowColor = 'rgba(255,255,255,0.35)';
      ctx.shadowBlur = 36;
      ctx.fill();

      ctx.lineWidth = 5;
      ctx.strokeStyle = 'rgba(255,255,255,0.7)';
      ctx.shadowBlur = 18;
      ctx.stroke();

      for (let i = 0; i < 6; i += 1) {
        const lift = -180 + i * 56 + Math.sin(time * 1.4 + i * 1.2) * 8;
        ctx.beginPath();
        ctx.strokeStyle = 'rgba(255,255,255,' + (0.24 - i * 0.03) + ')';
        ctx.lineWidth = 2.2;
        ctx.arc(-24 + i * 8, lift, 120 - i * 12, -1.1, -0.2);
        ctx.stroke();
      }

      ctx.restore();
      drawDust(time, 180, 330, 160, 0.42);
    }

    function drawCloudGate(time) {
      const cx = W * 0.5;
      const cy = H * 0.5;
      const baseR = Math.min(W, H) * 0.24;

      for (let i = 0; i < 94; i += 1) {
        const a = (i / 94) * TAU + time * 0.25;
        const drift = Math.sin(i * 0.71 + time * 2.1) * 18;
        const puffR = 22 + Math.sin(i * 1.9 - time * 1.7) * 8;
        const x = cx + Math.cos(a) * (baseR + drift);
        const y = cy + Math.sin(a) * (baseR + drift);

        const g = ctx.createRadialGradient(x, y, 0, x, y, puffR);
        g.addColorStop(0, 'rgba(255,255,255,0.26)');
        g.addColorStop(0.65, 'rgba(216,240,255,0.14)');
        g.addColorStop(1, 'rgba(216,240,255,0)');
        ctx.fillStyle = g;
        ctx.beginPath();
        ctx.arc(x, y, puffR, 0, TAU);
        ctx.fill();
      }

      ctx.save();
      ctx.translate(cx, cy);
      ctx.rotate(time * 0.22);

      if (typeof ctx.createConicGradient === 'function') {
        const cg = ctx.createConicGradient(0, 0, 0);
        cg.addColorStop(0, 'rgba(110,240,255,0.26)');
        cg.addColorStop(0.3, 'rgba(255,120,240,0.28)');
        cg.addColorStop(0.7, 'rgba(200,170,255,0.24)');
        cg.addColorStop(1, 'rgba(110,240,255,0.26)');
        ctx.fillStyle = cg;
      } else {
        const rg = ctx.createRadialGradient(0, 0, 20, 0, 0, baseR * 0.9);
        rg.addColorStop(0, 'rgba(255,255,255,0.25)');
        rg.addColorStop(1, 'rgba(150,220,255,0.1)');
        ctx.fillStyle = rg;
      }

      ctx.beginPath();
      ctx.arc(0, 0, baseR * 0.74, 0, TAU);
      ctx.fill();

      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(0, 0, baseR * 0.45, 0, TAU);
      ctx.fill();
      ctx.globalCompositeOperation = 'source-over';

      for (let i = 0; i < 3; i += 1) {
        ctx.beginPath();
        ctx.lineWidth = 2 + i;
        ctx.strokeStyle = hsla(190 + i * 45 + Math.sin(time * 2 + i) * 10, 100, 80, 0.24 - i * 0.05);
        ctx.setLineDash([12 + i * 8, 8 + i * 3]);
        ctx.lineDashOffset = -time * (28 + i * 14);
        ctx.arc(0, 0, baseR * (0.5 + i * 0.11), 0, TAU);
        ctx.stroke();
      }
      ctx.setLineDash([]);
      ctx.restore();

      drawDust(time, 180, 260, 170, 0.47);
    }

    function drawErrorWindowDrift(time) {
      const jitterX = Math.sin(time * 3.2) * W * 0.02;
      const jitterY = Math.cos(time * 2.6) * H * 0.015;
      const winW = W * 0.62;
      const winH = H * 0.44;
      const x = (W - winW) * 0.5 + jitterX;
      const y = (H - winH) * 0.46 + jitterY;
      const titleH = 62;

      const ghosts = [
        { dx: -7, dy: -2, col: 'rgba(0,255,255,0.17)' },
        { dx: 6, dy: 2, col: 'rgba(255,0,180,0.16)' },
      ];

      for (const g of ghosts) {
        roundedRectPath(x + g.dx, y + g.dy, winW, winH, 18);
        ctx.fillStyle = g.col;
        ctx.fill();
      }

      roundedRectPath(x, y, winW, winH, 18);
      const panelGrad = ctx.createLinearGradient(x, y, x, y + winH);
      panelGrad.addColorStop(0, 'rgba(255,255,255,0.21)');
      panelGrad.addColorStop(1, 'rgba(206,210,235,0.11)');
      ctx.fillStyle = panelGrad;
      ctx.shadowColor = 'rgba(130,220,255,0.35)';
      ctx.shadowBlur = 30;
      ctx.fill();

      roundedRectPath(x, y, winW, titleH, 18);
      const titleGrad = ctx.createLinearGradient(x, y, x + winW, y);
      titleGrad.addColorStop(0, 'rgba(0,210,255,0.75)');
      titleGrad.addColorStop(0.52, 'rgba(255,64,196,0.72)');
      titleGrad.addColorStop(1, 'rgba(165,120,255,0.74)');
      ctx.fillStyle = titleGrad;
      ctx.fill();

      const btnY = y + titleH * 0.5;
      const btnR = 8;
      ['#00ffd0', '#ffe066', '#ff5bb4'].forEach((c, i) => {
        ctx.beginPath();
        ctx.fillStyle = c;
        ctx.arc(x + winW - 34 - i * 24, btnY, btnR, 0, TAU);
        ctx.fill();
      });

      ctx.font = '700 34px "Courier New", monospace';
      ctx.fillStyle = 'rgba(255,255,255,0.95)';
      ctx.fillText('DREAMCORE.EXE', x + 24, y + 43);

      ctx.font = '700 56px "Courier New", monospace';
      const glitch = Math.sin(time * 18) * 6;
      ctx.fillStyle = 'rgba(255,255,255,0.92)';
      ctx.fillText('ERROR: REALITY NOT FOUND', x + 38, y + 168 + glitch * 0.2);
      ctx.fillStyle = 'rgba(0,255,255,0.4)';
      ctx.fillText('ERROR: REALITY NOT FOUND', x + 34 + glitch, y + 168);
      ctx.fillStyle = 'rgba(255,0,180,0.38)';
      ctx.fillText('ERROR: REALITY NOT FOUND', x + 43 - glitch, y + 171);

      ctx.font = '500 30px "Courier New", monospace';
      ctx.fillStyle = 'rgba(220,240,255,0.82)';
      ctx.fillText('fatal exception in memory://angelic', x + 44, y + 232);
      ctx.fillText('press any key to keep dreaming', x + 44, y + 276);

      for (let sy = y + titleH + 8; sy < y + winH - 8; sy += 4) {
        ctx.fillStyle = 'rgba(255,255,255,0.03)';
        ctx.fillRect(x + 8, sy + Math.sin(time * 10 + sy * 0.01) * 0.6, winW - 16, 1);
      }

      drawDust(time, 180, 330, 140, 0.47);
    }

    function drawWireframeStarTrails(time) {
      const cx = W * 0.5;
      const cy = H * 0.5;

      for (let k = 8; k >= 0; k -= 1) {
        const phase = time - k * 0.055;
        const rot = phase * 2.9;
        const outer = Math.min(W, H) * (0.13 + k * 0.01);
        const inner = outer * 0.45;
        const pts = starPoints(cx, cy, outer, inner, 5, rot);
        const alpha = 0.06 + (1 - k / 9) * 0.33;
        const hue = 105 + k * 4 + Math.sin(time * 1.6 + k) * 8;

        strokePath(pts, hsla(hue, 95, 74, 1), 2.2, alpha, 18);

        ctx.save();
        ctx.strokeStyle = hsla(145, 100, 86, alpha * 0.72);
        ctx.lineWidth = 1.2;
        ctx.beginPath();
        for (let i = 0; i < pts.length; i += 2) {
          const a = pts[i];
          const b = pts[(i + 4) % pts.length];
          ctx.moveTo(a.x, a.y);
          ctx.lineTo(b.x, b.y);
        }
        ctx.stroke();
        ctx.restore();
      }

      const flare = ctx.createRadialGradient(cx, cy, 0, cx, cy, Math.min(W, H) * 0.1);
      flare.addColorStop(0, 'rgba(190,255,210,0.42)');
      flare.addColorStop(1, 'rgba(190,255,210,0)');
      ctx.fillStyle = flare;
      ctx.beginPath();
      ctx.arc(cx, cy, Math.min(W, H) * 0.1, 0, TAU);
      ctx.fill();

      drawDust(time, 120, 180, 155, 0.45);
    }

    function dolphinShape(scale) {
      ctx.beginPath();
      ctx.moveTo(-220 * scale, 44 * scale);
      ctx.bezierCurveTo(-148 * scale, -88 * scale, 88 * scale, -82 * scale, 218 * scale, 12 * scale);
      ctx.bezierCurveTo(268 * scale, 44 * scale, 252 * scale, 102 * scale, 188 * scale, 92 * scale);
      ctx.bezierCurveTo(126 * scale, 86 * scale, 78 * scale, 62 * scale, 18 * scale, 36 * scale);
      ctx.bezierCurveTo(44 * scale, 96 * scale, 10 * scale, 130 * scale, -42 * scale, 120 * scale);
      ctx.bezierCurveTo(-94 * scale, 110 * scale, -126 * scale, 86 * scale, -160 * scale, 62 * scale);
      ctx.bezierCurveTo(-182 * scale, 70 * scale, -206 * scale, 66 * scale, -220 * scale, 44 * scale);
      ctx.closePath();
    }

    function drawDolphinPrism(time) {
      const cx = W * 0.5;
      const cy = H * 0.53;
      const scale = Math.min(W, H) / 1080;

      ctx.save();
      ctx.translate(cx, cy);

      const spectrum = [
        'rgba(255,100,210,0.2)',
        'rgba(255,165,90,0.2)',
        'rgba(255,236,120,0.2)',
        'rgba(120,255,205,0.2)',
        'rgba(95,210,255,0.2)',
        'rgba(180,140,255,0.2)',
      ];

      for (let i = 0; i < spectrum.length; i += 1) {
        const y = -140 * scale + i * 44 * scale + Math.sin(time * 2 + i * 0.8) * 8;
        const spread = 360 * scale;
        ctx.beginPath();
        ctx.fillStyle = spectrum[i];
        ctx.moveTo(-40 * scale, y - 18 * scale);
        ctx.lineTo(spread, y - 54 * scale);
        ctx.lineTo(spread, y + 54 * scale);
        ctx.lineTo(-40 * scale, y + 18 * scale);
        ctx.closePath();
        ctx.fill();
      }

      dolphinShape(scale);
      const bodyGrad = ctx.createLinearGradient(-220 * scale, -110 * scale, 210 * scale, 120 * scale);
      bodyGrad.addColorStop(0, 'rgba(255,255,255,0.95)');
      bodyGrad.addColorStop(0.5, 'rgba(185,230,255,0.92)');
      bodyGrad.addColorStop(1, 'rgba(240,190,255,0.9)');
      ctx.fillStyle = bodyGrad;
      ctx.shadowColor = 'rgba(180,240,255,0.45)';
      ctx.shadowBlur = 26;
      ctx.fill();

      ctx.save();
      dolphinShape(scale);
      ctx.clip();
      for (let i = -8; i < 10; i += 1) {
        const x = i * 42 * scale + (time * 110) % (42 * scale);
        ctx.fillStyle = hsla(170 + i * 14, 95, 80, 0.18);
        ctx.fillRect(x - 22 * scale, -180 * scale, 18 * scale, 360 * scale);
      }
      ctx.restore();

      ctx.lineWidth = 3;
      ctx.strokeStyle = 'rgba(255,255,255,0.72)';
      dolphinShape(scale);
      ctx.stroke();

      ctx.restore();
      drawDust(time, 170, 315, 160, 0.48);
    }

    const BAYER4 = [
      [0 / 16, 8 / 16, 2 / 16, 10 / 16],
      [12 / 16, 4 / 16, 14 / 16, 6 / 16],
      [3 / 16, 11 / 16, 1 / 16, 9 / 16],
      [15 / 16, 7 / 16, 13 / 16, 5 / 16],
    ];

    function drawCelestialDitherSun(time) {
      const cx = W * 0.5;
      const cy = H * 0.5;
      const r = Math.min(W, H) * 0.22;
      const step = Math.max(4, Math.floor(Math.min(W, H) / 220));

      const halo = ctx.createRadialGradient(cx, cy, r * 0.4, cx, cy, r * 1.55);
      halo.addColorStop(0, 'rgba(255,240,160,0.23)');
      halo.addColorStop(1, 'rgba(255,240,160,0)');
      ctx.fillStyle = halo;
      ctx.beginPath();
      ctx.arc(cx, cy, r * 1.55, 0, TAU);
      ctx.fill();

      for (let y = Math.floor(cy - r - 48); y <= Math.ceil(cy + r + 48); y += step) {
        for (let x = Math.floor(cx - r - 48); x <= Math.ceil(cx + r + 48); x += step) {
          const dx = x - cx;
          const dy = y - cy;
          const d = Math.sqrt(dx * dx + dy * dy);
          const edge = clamp((r - d) / 34, 0, 1);
          if (edge <= 0) continue;
          const by = ((y / step) | 0) & 3;
          const bx = ((x / step) | 0) & 3;
          const threshold = BAYER4[by][bx];
          if (edge > threshold) {
            const hue = 34 + Math.sin(time * 1.4 + d * 0.025) * 18;
            ctx.fillStyle = hsla(hue, 100, 73, clamp(edge * 0.9, 0, 0.9));
            ctx.fillRect(x, y, step, step);
          }
        }
      }

      for (let i = 0; i < 36; i += 1) {
        const a = (i / 36) * TAU + time * 0.24;
        const inner = r * 1.05;
        const outer = r * (1.26 + (i % 3) * 0.08);
        const x1 = cx + Math.cos(a) * inner;
        const y1 = cy + Math.sin(a) * inner;
        const x2 = cx + Math.cos(a) * outer;
        const y2 = cy + Math.sin(a) * outer;
        ctx.strokeStyle = hsla(48 + i * 2, 100, 82, 0.24);
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x2, y2);
        ctx.stroke();
      }

      drawDust(time, 28, 62, 180, 0.46);
    }

    function drawY2KTechRing(time) {
      const cx = W * 0.5;
      const cy = H * 0.5;

      for (let ring = 0; ring < 5; ring += 1) {
        const radius = Math.min(W, H) * (0.11 + ring * 0.055);
        const segments = 14 + ring * 6;
        const rot = time * (ring % 2 === 0 ? 0.9 : -0.75) + ring * 0.31;

        for (let s = 0; s < segments; s += 1) {
          if ((s + Math.floor(time * 3 + ring)) % 3 === 0) continue;
          const a0 = rot + (s / segments) * TAU;
          const a1 = a0 + TAU / segments * 0.7;
          ctx.beginPath();
          ctx.lineWidth = 4 - ring * 0.4;
          ctx.strokeStyle = hsla(170 + ring * 22 + s * 0.8, 96, 74, 0.3);
          ctx.shadowColor = 'rgba(100,230,255,0.42)';
          ctx.shadowBlur = 14;
          ctx.arc(cx, cy, radius, a0, a1);
          ctx.stroke();
        }
      }

      for (let i = 0; i < 24; i += 1) {
        const a = (i / 24) * TAU - time * 1.6;
        const r = Math.min(W, H) * 0.305;
        const x = cx + Math.cos(a) * r;
        const y = cy + Math.sin(a) * r;
        const size = 2 + (i % 3);
        ctx.beginPath();
        ctx.fillStyle = hsla(190 + i * 3, 100, 82, 0.4);
        ctx.arc(x, y, size, 0, TAU);
        ctx.fill();
      }

      ctx.beginPath();
      ctx.strokeStyle = 'rgba(255,255,255,0.55)';
      ctx.lineWidth = 2;
      ctx.arc(cx, cy, Math.min(W, H) * 0.07, 0, TAU);
      ctx.stroke();

      drawDust(time, 170, 240, 165, 0.43);
    }

    function drawCdromRainbow(time) {
      const cx = W * 0.5;
      const cy = H * 0.5;
      const outer = Math.min(W, H) * 0.29;
      const inner = Math.min(W, H) * 0.06;

      ctx.save();
      ctx.translate(cx, cy);

      if (typeof ctx.createConicGradient === 'function') {
        ctx.rotate(time * 0.55);
        const cg = ctx.createConicGradient(0, 0, 0);
        cg.addColorStop(0, 'rgba(255,130,235,0.36)');
        cg.addColorStop(0.17, 'rgba(130,220,255,0.38)');
        cg.addColorStop(0.34, 'rgba(120,255,205,0.36)');
        cg.addColorStop(0.5, 'rgba(255,255,150,0.35)');
        cg.addColorStop(0.67, 'rgba(255,170,120,0.36)');
        cg.addColorStop(0.84, 'rgba(165,150,255,0.36)');
        cg.addColorStop(1, 'rgba(255,130,235,0.36)');
        ctx.fillStyle = cg;
      } else {
        const rg = ctx.createRadialGradient(0, 0, inner, 0, 0, outer);
        rg.addColorStop(0, 'rgba(240,240,255,0.2)');
        rg.addColorStop(1, 'rgba(170,220,255,0.32)');
        ctx.fillStyle = rg;
      }

      ctx.beginPath();
      ctx.arc(0, 0, outer, 0, TAU);
      ctx.fill();

      const metallic = ctx.createRadialGradient(-outer * 0.2, -outer * 0.25, inner * 0.2, 0, 0, outer);
      metallic.addColorStop(0, 'rgba(255,255,255,0.45)');
      metallic.addColorStop(0.5, 'rgba(185,195,225,0.18)');
      metallic.addColorStop(1, 'rgba(130,145,180,0.06)');
      ctx.fillStyle = metallic;
      ctx.beginPath();
      ctx.arc(0, 0, outer, 0, TAU);
      ctx.fill();

      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(0, 0, inner, 0, TAU);
      ctx.fill();
      ctx.globalCompositeOperation = 'source-over';

      for (let i = 0; i < 4; i += 1) {
        ctx.save();
        ctx.rotate(time * 0.75 + i * (Math.PI / 2));
        ctx.fillStyle = 'rgba(255,255,255,0.15)';
        ctx.fillRect(-6, -outer, 12, outer * 0.9);
        ctx.restore();
      }

      ctx.lineWidth = 3;
      ctx.strokeStyle = 'rgba(255,255,255,0.55)';
      ctx.beginPath();
      ctx.arc(0, 0, outer - 2, 0, TAU);
      ctx.stroke();

      ctx.restore();
      drawDust(time, 150, 330, 180, 0.5);
    }

    function drawFallingDataWater(time) {
      const columns = 130;
      const wStep = W / columns;

      for (let i = 0; i < columns; i += 1) {
        const seed = i * 7.13;
        const speed = 0.22 + hash(seed + 1.1) * 1.05;
        const phase = hash(seed + 3.7);
        const y = ((time * speed + phase) % 1.25) * (H * 1.12) - H * 0.12;
        const len = H * (0.04 + hash(seed + 2.2) * 0.13);
        const x = i * wStep + wStep * 0.5 + Math.sin(seed) * 4;
        const hue = 170 + hash(seed + 8.2) * 45;

        const grad = ctx.createLinearGradient(x, y - len, x, y);
        grad.addColorStop(0, hsla(hue, 96, 74, 0));
        grad.addColorStop(0.6, hsla(hue + 20, 98, 80, 0.18));
        grad.addColorStop(1, hsla(hue, 100, 84, 0.52));

        ctx.strokeStyle = grad;
        ctx.lineWidth = 1.2 + hash(seed + 5.5) * 2.2;
        ctx.beginPath();
        ctx.moveTo(x, y - len);
        ctx.lineTo(x, y);
        ctx.stroke();

        ctx.beginPath();
        ctx.fillStyle = hsla(hue + 10, 100, 88, 0.45);
        ctx.arc(x, y, 1.4 + hash(seed + 6.3) * 3.2, 0, TAU);
        ctx.fill();

        if (i % 7 === 0) {
          for (let j = 0; j < 4; j += 1) {
            const gy = y - j * 12 - hash(seed + j * 2.1) * 4;
            ctx.fillStyle = hsla(180 + j * 8, 100, 86, 0.16 - j * 0.03);
            ctx.fillRect(x - 2, gy, 4, 6);
          }
        }
      }

      drawDust(time, 170, 220, 140, 0.5);
    }

    function drawButterflyPulse(time) {
      const cx = W * 0.5;
      const cy = H * 0.54;
      const flap = 0.78 + Math.sin(time * 8.2) * 0.26;
      const scale = Math.min(W, H) / 1080;

      ctx.save();
      ctx.translate(cx, cy);

      function wing(side) {
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.bezierCurveTo(120 * side * scale, -130 * flap * scale, 260 * side * scale, -70 * flap * scale, 220 * side * scale, 64 * flap * scale);
        ctx.bezierCurveTo(170 * side * scale, 190 * flap * scale, 78 * side * scale, 162 * flap * scale, 0, 20 * flap * scale);
        ctx.closePath();
      }

      const wingGrad = ctx.createLinearGradient(-260 * scale, -120 * scale, 260 * scale, 170 * scale);
      wingGrad.addColorStop(0, 'rgba(255,70,210,0.7)');
      wingGrad.addColorStop(0.5, 'rgba(255,130,245,0.74)');
      wingGrad.addColorStop(1, 'rgba(150,180,255,0.62)');

      ctx.fillStyle = wingGrad;
      ctx.shadowColor = 'rgba(255,80,210,0.45)';
      ctx.shadowBlur = 30;
      wing(-1);
      ctx.fill();
      wing(1);
      ctx.fill();

      ctx.lineWidth = 3;
      ctx.strokeStyle = 'rgba(255,255,255,0.7)';
      wing(-1);
      ctx.stroke();
      wing(1);
      ctx.stroke();

      ctx.strokeStyle = 'rgba(255,255,255,0.72)';
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(0, -34 * scale);
      ctx.lineTo(0, 122 * scale);
      ctx.stroke();

      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(-8 * scale, -26 * scale);
      ctx.quadraticCurveTo(-36 * scale, -84 * scale, -84 * scale, -108 * scale);
      ctx.moveTo(8 * scale, -26 * scale);
      ctx.quadraticCurveTo(36 * scale, -84 * scale, 84 * scale, -108 * scale);
      ctx.stroke();

      ctx.restore();
      drawDust(time, 280, 340, 190, 0.48);
    }

    function renderFrame(payload) {
      const motifId = payload.motifId;
      const frame = payload.frame;
      const fps = payload.fps;
      const duration = payload.duration;
      const width = payload.width;
      const height = payload.height;

      setSize(width, height);
      clearFrame();

      const time = frame / fps;
      const t = time / duration;

      if (motifId === '01_iridescent_chrome_heart') {
        drawIridescentChromeHeart(time);
      } else if (motifId === '02_cloud_gate') {
        drawCloudGate(time);
      } else if (motifId === '03_error_window_drift') {
        drawErrorWindowDrift(time);
      } else if (motifId === '04_wireframe_star_trails') {
        drawWireframeStarTrails(time);
      } else if (motifId === '05_dolphin_prism') {
        drawDolphinPrism(time);
      } else if (motifId === '06_celestial_dither_sun') {
        drawCelestialDitherSun(time);
      } else if (motifId === '07_y2k_tech_ring') {
        drawY2KTechRing(time);
      } else if (motifId === '08_cdrom_rainbow') {
        drawCdromRainbow(time);
      } else if (motifId === '09_falling_data_water') {
        drawFallingDataWater(time);
      } else if (motifId === '10_butterfly_pulse') {
        drawButterflyPulse(time);
      } else {
        const cx = W * 0.5;
        const cy = H * 0.5;
        const r = Math.min(W, H) * 0.2;
        ctx.fillStyle = hsla(200 + t * 80, 95, 70, 0.5);
        ctx.beginPath();
        ctx.arc(cx, cy, r, 0, TAU);
        ctx.fill();
      }

      return canvas.toDataURL('image/png');
    }

    window.renderFrame = renderFrame;
    window.rendererReady = true;
  </script>
</body>
</html>`;
}

async function renderMotif(page, motif, config, report) {
  const totalFrames = Math.round(config.fps * config.duration);
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), `${motif.id}-`));
  const outputPath = path.join(config.outDir, `${motif.id}.mov`);

  console.log(`\n[${motif.id}] Rendering ${totalFrames} frames...`);

  for (let frame = 0; frame < totalFrames; frame += 1) {
    const dataUrl = await page.evaluate((payload) => window.renderFrame(payload), {
      motifId: motif.id,
      frame,
      fps: config.fps,
      duration: config.duration,
      width: config.width,
      height: config.height,
    });

    const base64 = dataUrl.replace(/^data:image\/png;base64,/, '');
    const buffer = Buffer.from(base64, 'base64');
    const frameName = `frame_${String(frame).padStart(4, '0')}.png`;
    fs.writeFileSync(path.join(tempDir, frameName), buffer);

    if (frame % Math.max(30, Math.floor(config.fps / 2)) === 0) {
      console.log(`[${motif.id}] frame ${frame}/${totalFrames}`);
    }
  }

  console.log(`[${motif.id}] Encoding MOV (ProRes 4444, yuva444p12le, vendor apl0)...`);
  encodeProRes4444(tempDir, config.fps, outputPath);

  const probe = probeVideo(outputPath);
  report.push({
    motif_id: motif.id,
    motif_label: motif.label,
    output: outputPath,
    stream: probe,
  });

  if (!config.keepFrames) {
    fs.rmSync(tempDir, { recursive: true, force: true });
  } else {
    console.log(`[${motif.id}] Kept frames: ${tempDir}`);
  }

  console.log(
    `[${motif.id}] Done -> ${outputPath} | pix_fmt=${probe.pix_fmt || 'unknown'} codec_tag=${probe.codec_tag_string || 'unknown'}`,
  );
}

async function main() {
  const config = parseArgs(process.argv.slice(2));
  fs.mkdirSync(config.outDir, { recursive: true });

  const selected =
    config.motifs.length === 0
      ? MOTIFS
      : MOTIFS.filter((m) => config.motifs.includes(m.id));

  if (selected.length === 0) {
    throw new Error('No motifs selected. Use --motif with a valid motif id.');
  }

  const unresolved = config.motifs.filter((id) => !MOTIFS.some((m) => m.id === id));
  if (unresolved.length > 0) {
    throw new Error(`Unknown motif id(s): ${unresolved.join(', ')}`);
  }

  console.log('Dreamcore/Y2K Pack Renderer');
  console.log(`Output dir: ${config.outDir}`);
  console.log(`Resolution: ${config.width}x${config.height}`);
  console.log(`FPS: ${config.fps}`);
  console.log(`Duration per asset: ${config.duration}s`);
  console.log(`Assets: ${selected.length}`);

  const browser = await puppeteer.launch({
    headless: true,
    args: ['--no-sandbox', '--disable-setuid-sandbox'],
  });

  const page = await browser.newPage();
  await page.setViewport({ width: config.width, height: config.height });
  await page.setContent(buildRuntimeHtml(), { waitUntil: 'load' });
  await page.waitForFunction(() => window.rendererReady === true);

  const report = [];
  for (const motif of selected) {
    await renderMotif(page, motif, config, report);
  }

  await browser.close();

  const reportPath = path.join(config.outDir, 'render_report.json');
  fs.writeFileSync(reportPath, JSON.stringify(report, null, 2));

  console.log('\nPack complete.');
  console.log(`Report: ${reportPath}`);
}

main().catch((error) => {
  console.error(`ERROR: ${error.message}`);
  process.exit(1);
});
