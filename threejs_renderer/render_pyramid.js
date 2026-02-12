#!/usr/bin/env node

import puppeteer from 'puppeteer';
import fs from 'fs';

// Parse CLI arguments
const args = process.argv.slice(2);
const config = {
  width: 1920,
  height: 1080,
  fps: 30,
  duration: 8,
  output: 'pyramid',
  test: false,
  frame: null
};

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--width') config.width = parseInt(args[++i]);
  if (args[i] === '--height') config.height = parseInt(args[++i]);
  if (args[i] === '--fps') config.fps = parseInt(args[++i]);
  if (args[i] === '--duration') config.duration = parseFloat(args[++i]);
  if (args[i] === '--output') config.output = args[++i];
  if (args[i] === '--test') config.test = true;
  if (args[i] === '--frame') config.frame = parseInt(args[++i]);
}

const totalFrames = config.test && config.frame !== null ? 1 : Math.floor(config.fps * config.duration);
const startFrame = config.test && config.frame !== null ? config.frame : 0;
const endFrame = config.test && config.frame !== null ? config.frame + 1 : totalFrames;

console.error(`Rendering ${totalFrames} frames at ${config.width}x${config.height} ${config.fps}fps`);

// Launch headless browser
const browser = await puppeteer.launch({
  headless: true,
  args: ['--no-sandbox', '--disable-setuid-sandbox']
});

const page = await browser.newPage();
await page.setViewport({ width: config.width, height: config.height });

// Create HTML page with ThreeJS
const html = `
<!DOCTYPE html>
<html>
<head>
  <style>
    body { margin: 0; overflow: hidden; }
    canvas { display: block; }
  </style>
</head>
<body>
  <script type="importmap">
    {
      "imports": {
        "three": "https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.module.js"
      }
    }
  </script>
  <script type="module">
    import * as THREE from 'three';
    
    // Create scene
    const scene = new THREE.Scene();
    
    // Create camera
    const camera = new THREE.PerspectiveCamera(
      45,
      ${config.width} / ${config.height},
      0.1,
      1000
    );
    camera.position.set(0, 2, 5);
    camera.lookAt(0, 0, 0);
    
    // Create renderer
    const renderer = new THREE.WebGLRenderer({ 
      alpha: true,
      antialias: true,
      preserveDrawingBuffer: true
    });
    renderer.setSize(${config.width}, ${config.height});
    renderer.setClearColor(0x000000, 0);
    document.body.appendChild(renderer.domElement);
    
    // Create pyramid geometry (tetrahedron)
    const pyramidGeometry = new THREE.TetrahedronGeometry(1.5, 0);
    const pyramidMaterial = new THREE.MeshPhongMaterial({
      color: 0xffffff,
      flatShading: true,
      side: THREE.DoubleSide
    });
    const pyramid = new THREE.Mesh(pyramidGeometry, pyramidMaterial);
    scene.add(pyramid);
    
    // Add lighting
    const ambientLight = new THREE.AmbientLight(0xffffff, 0.4);
    scene.add(ambientLight);
    
    const directionalLight = new THREE.DirectionalLight(0xffffff, 0.8);
    directionalLight.position.set(5, 10, 7.5);
    scene.add(directionalLight);
    
    // Expose render function
    window.renderFrame = function(frameNum, fps) {
      const time = frameNum / fps;
      
      // Animate pyramid rotation
      pyramid.rotation.y = time * 0.8;
      pyramid.rotation.x = Math.sin(time * 0.3) * 0.2;
      
      // Apply 90s dreamcore color gradient
      const gradientT = 0.5 + Math.sin(time * 0.5) * 0.5;
      const colorTop = new THREE.Color(0.85, 0.75, 0.95);
      const colorBottom = new THREE.Color(0.75, 0.85, 0.9);
      const currentColor = new THREE.Color().lerpColors(colorBottom, colorTop, gradientT);
      pyramidMaterial.color = currentColor;
      
      // Render
      renderer.render(scene, camera);
      
      // Return canvas data URL
      return renderer.domElement.toDataURL('image/png');
    };
    
    // Signal ready
    window.rendererReady = true;
  </script>
</body>
</html>
`;

await page.setContent(html);

// Wait for renderer to be ready
await page.waitForFunction(() => window.rendererReady);

// Render frames
for (let frame = startFrame; frame < endFrame; frame++) {
  const dataUrl = await page.evaluate((frameNum, fps) => {
    return window.renderFrame(frameNum, fps);
  }, frame, config.fps);

  if (config.test) {
    // Save as PNG for testing
    const base64Data = dataUrl.replace(/^data:image\/png;base64,/, '');
    const buffer = Buffer.from(base64Data, 'base64');
    const filename = `${config.output}_frame${frame}.png`;
    fs.writeFileSync(filename, buffer);
    console.error(`Wrote ${filename}`);
  } else {
    // Save as sequential PNG files for FFmpeg
    const base64Data = dataUrl.replace(/^data:image\/png;base64,/, '');
    const buffer = Buffer.from(base64Data, 'base64');
    const frameNum = String(frame).padStart(4, '0');
    const filename = `${config.output}${frameNum}.png`;
    fs.writeFileSync(filename, buffer);
  }

  if (frame % 30 === 0) {
    console.error(`Rendered frame ${frame}/${totalFrames}`);
  }
}

await browser.close();
console.error('Rendering complete');
process.exit(0);
