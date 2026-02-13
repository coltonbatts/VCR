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
    
    // Create lightning bolts
    const lightningBolts = [];
    const numBolts = 4;
    
    for (let i = 0; i < numBolts; i++) {
      const points = [];
      // Will be updated per frame
      for (let j = 0; j < 10; j++) {
        points.push(new THREE.Vector3(0, 0, 0));
      }
      
      const geometry = new THREE.BufferGeometry().setFromPoints(points);
      const material = new THREE.LineBasicMaterial({
        color: 0x00ffff,
        linewidth: 2,
        transparent: true,
        opacity: 0.8
      });
      
      const lightning = new THREE.Line(geometry, material);
      scene.add(lightning);
      lightningBolts.push({
        line: lightning,
        geometry: geometry,
        material: material,
        active: false,
        timer: Math.random() * 2
      });
    }
    
    // Function to generate lightning path
    function generateLightningPath(start, end, segments = 10, chaos = 0.3) {
      const points = [];
      points.push(start.clone());
      
      for (let i = 1; i < segments - 1; i++) {
        const t = i / (segments - 1);
        const point = new THREE.Vector3().lerpVectors(start, end, t);
        
        // Add random offset perpendicular to the line
        const offset = new THREE.Vector3(
          (Math.random() - 0.5) * chaos,
          (Math.random() - 0.5) * chaos,
          (Math.random() - 0.5) * chaos
        );
        point.add(offset);
        points.push(point);
      }
      
      points.push(end.clone());
      return points;
    }
    
    // Enhanced lighting setup
    // Soft ambient base
    const ambientLight = new THREE.AmbientLight(0x9090b0, 0.3);
    scene.add(ambientLight);
    
    // Main directional light (soft white)
    const directionalLight = new THREE.DirectionalLight(0xffffff, 0.6);
    directionalLight.position.set(5, 10, 7.5);
    scene.add(directionalLight);
    
    // Rim light from behind (creates edge glow)
    const rimLight = new THREE.DirectionalLight(0xd0c0ff, 0.5);
    rimLight.position.set(-3, 2, -5);
    scene.add(rimLight);
    
    // Colored point lights for atmosphere (will move)
    const pointLight1 = new THREE.PointLight(0xff80c0, 0.8, 15);
    pointLight1.position.set(3, 3, 3);
    scene.add(pointLight1);
    
    const pointLight2 = new THREE.PointLight(0x80c0ff, 0.8, 15);
    pointLight2.position.set(-3, 3, 3);
    scene.add(pointLight2);

    
    // Expose render function
    window.renderFrame = function(frameNum, fps) {
      const time = frameNum / fps;
      
      // Animate pyramid rotation
      pyramid.rotation.y = time * 0.8;
      pyramid.rotation.x = Math.sin(time * 0.3) * 0.2;
      
      // Animate colored point lights in circular motion
      const lightRadius = 5;
      pointLight1.position.x = Math.cos(time * 0.5) * lightRadius;
      pointLight1.position.z = Math.sin(time * 0.5) * lightRadius;
      pointLight1.position.y = 3 + Math.sin(time * 0.7) * 1;
      
      pointLight2.position.x = Math.cos(time * 0.5 + Math.PI) * lightRadius;
      pointLight2.position.z = Math.sin(time * 0.5 + Math.PI) * lightRadius;
      pointLight2.position.y = 3 + Math.cos(time * 0.7) * 1;
      
      // Update lightning bolts
      const pyramidVertices = pyramidGeometry.attributes.position.array;
      
      lightningBolts.forEach((bolt, index) => {
        bolt.timer -= 1/fps;
        
        if (bolt.timer <= 0) {
          // Activate bolt
          bolt.active = true;
          bolt.timer = 0.1 + Math.random() * 0.3; // Flash duration
          
          // Pick a random vertex from the pyramid
          const vertexIndex = Math.floor(Math.random() * (pyramidVertices.length / 3)) * 3;
          const startPos = new THREE.Vector3(
            pyramidVertices[vertexIndex],
            pyramidVertices[vertexIndex + 1],
            pyramidVertices[vertexIndex + 2]
          );
          
          // Apply pyramid's rotation to vertex
          startPos.applyMatrix4(pyramid.matrixWorld);
          
          // Random endpoint in space
          const endPos = new THREE.Vector3(
            (Math.random() - 0.5) * 8,
            Math.random() * 6 - 1,
            (Math.random() - 0.5) * 8
          );
          
          // Generate jagged lightning path
          const points = generateLightningPath(startPos, endPos, 12, 0.4);
          bolt.geometry.setFromPoints(points);
          
          // Random color - cyan or magenta
          const color = Math.random() > 0.5 ? 0x00ffff : 0xff00ff;
          bolt.material.color.setHex(color);
          bolt.material.opacity = 0.7 + Math.random() * 0.3;
        } else if (bolt.timer < 0.05) {
          // Flicker effect near end
          bolt.material.opacity = Math.random() * 0.5;
        }
        
        // Deactivate when timer runs out
        if (bolt.timer <= -0.5) {
          bolt.active = false;
          bolt.timer = 0.5 + Math.random() * 2; // Wait before next strike
          bolt.material.opacity = 0;
        }
      });
      
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
