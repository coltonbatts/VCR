#!/usr/bin/env node

import puppeteer from 'puppeteer';
import fs from 'fs';

// Parse CLI arguments
const config = {
  width: 720,
  height: 1280,
  fps: 30,
  duration: 5,
  output: 'dreamcore_circle',
  test: false,
  frame: null
};

const args = process.argv.slice(2);
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

// Pipe console logs
page.on('console', msg => console.error('PAGE LOG:', msg.text()));

// Create HTML page with ThreeJS
const html = `
<!DOCTYPE html>
<html>
<head>
  <style>
    body { margin: 0; overflow: hidden; background-color: #000; }
    canvas { display: block; }
  </style>
</head>
<body>
  <script type="importmap">
    {
      "imports": {
        "three": "https://cdn.jsdelivr.net/npm/three@0.160.0/build/three.module.js",
        "three/addons/": "https://cdn.jsdelivr.net/npm/three@0.160.0/examples/jsm/"
      }
    }
  </script>
  <script type="module">
    import * as THREE from 'three';
    
    // Create scene
    const scene = new THREE.Scene();
    
    // Create camera
    const camera = new THREE.PerspectiveCamera(
      60,
      ${config.width} / ${config.height},
      0.1,
      1000
    );
    camera.position.set(0, 0, 8);
    camera.lookAt(0, 0, 0);
    
    // Create renderer
    const renderer = new THREE.WebGLRenderer({ 
      alpha: true,
      antialias: false,
      preserveDrawingBuffer: true
    });
    renderer.setSize(${config.width}, ${config.height});
    renderer.setPixelRatio(1);
    document.body.appendChild(renderer.domElement);
    
    // Post Processing Setup
    const renderTarget = new THREE.WebGLRenderTarget(${config.width}, ${config.height});
    const postScene = new THREE.Scene();
    const postCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    const postGeometry = new THREE.PlaneGeometry(2, 2);
    
    const postMaterial = new THREE.ShaderMaterial({
      uniforms: {
        tDiffuse: { value: null },
        time: { value: 0 },
        resolution: { value: new THREE.Vector2(${config.width}, ${config.height}) }
      },
      vertexShader: \`
        varying vec2 vUv;
        void main() {
          vUv = uv;
          gl_Position = vec4(position, 1.0);
        }
      \`,
      fragmentShader: \`
        uniform sampler2D tDiffuse;
        uniform float time;
        uniform vec2 resolution;
        varying vec2 vUv;
        
        float bayer4(vec2 p) {
            vec2 p_mod = mod(p, 4.0);
            int x = int(p_mod.x);
            int y = int(p_mod.y);
            int index = x + y * 4;
            if (index == 0) return 0.0/16.0;
            if (index == 1) return 8.0/16.0;
            if (index == 2) return 2.0/16.0;
            if (index == 3) return 10.0/16.0;
            if (index == 4) return 12.0/16.0;
            if (index == 5) return 4.0/16.0;
            if (index == 6) return 14.0/16.0;
            if (index == 7) return 6.0/16.0;
            if (index == 8) return 3.0/16.0;
            if (index == 9) return 11.0/16.0;
            if (index == 10) return 1.0/16.0;
            if (index == 11) return 9.0/16.0;
            if (index == 12) return 15.0/16.0;
            if (index == 13) return 7.0/16.0;
            if (index == 14) return 13.0/16.0;
            if (index == 15) return 5.0/16.0;
            return 0.0;
        }

        void main() {
            vec4 col = texture2D(tDiffuse, vUv);
            
            // Posterize
            float levels = 12.0;
            vec3 posterized = floor(col.rgb * levels) / levels;
            
            // Dither
            float threshold = bayer4(gl_FragCoord.xy);
            vec3 finalCol = posterized + step(vec3(threshold), col.rgb - posterized) * (1.0/levels);
            
            // Grain
            float noise = (fract(sin(dot(vUv + time, vec2(12.9898, 78.233))) * 43758.5453) - 0.5) * 0.05;
            finalCol += noise;
            
            gl_FragColor = vec4(finalCol, col.a);
        }
      \`
    });
    const postMesh = new THREE.Mesh(postGeometry, postMaterial);
    postScene.add(postMesh);
    
    // Geometry
    const ringGeometry = new THREE.TorusGeometry(2, 0.15, 32, 128);
    const ringMaterial = new THREE.MeshStandardMaterial({
      color: 0xffffff,
      emissive: 0x4400ff,
      metalness: 0.9,
      roughness: 0.1
    });
    const ring = new THREE.Mesh(ringGeometry, ringMaterial);
    scene.add(ring);
    
    // Improved Fire Shader
    const fireVertexShader = \`
      varying vec2 vUv;
      uniform float time;
      void main() {
        vUv = uv;
        vec3 pos = position;
        float h = uv.y;
        pos.x += sin(pos.y * 10.0 + time * 10.0) * 0.1 * h;
        pos.z += cos(pos.y * 10.0 + time * 8.0) * 0.1 * h;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(pos, 1.0);
      }
    \`;
    const fireFragmentShader = \`
      varying vec2 vUv;
      uniform float time;
      void main() {
        float alpha = (1.0 - vUv.y) * 0.8;
        vec3 col = mix(vec3(0.0, 0.8, 1.0), vec3(1.0, 0.0, 1.0), vUv.y);
        col = mix(col, vec3(1.0), pow(1.0 - vUv.y, 4.0));
        gl_FragColor = vec4(col, alpha);
      }
    \`;
    const fireMaterial = new THREE.ShaderMaterial({
      vertexShader: fireVertexShader,
      fragmentShader: fireFragmentShader,
      transparent: true,
      side: THREE.DoubleSide,
      blending: THREE.AdditiveBlending,
      uniforms: { time: { value: 0 } }
    });
    
    const fireGroup = new THREE.Group();
    for (let i = 0; i < 32; i++) {
        const fireGeo = new THREE.ConeGeometry(0.2, 1.5, 4, 8, true);
        const fire = new THREE.Mesh(fireGeo, fireMaterial);
        const angle = (i / 32) * Math.PI * 2;
        fire.position.set(Math.cos(angle) * 2, Math.sin(angle) * 2, 0);
        fire.rotation.z = angle - Math.PI / 2;
        fire.rotation.x = Math.PI / 2;
        fireGroup.add(fire);
    }
    scene.add(fireGroup);
    
    const light1 = new THREE.PointLight(0x00ffff, 5, 20);
    light1.position.set(2, 2, 2);
    scene.add(light1);
    const light2 = new THREE.PointLight(0xff00ff, 5, 20);
    light2.position.set(-2, -2, 2);
    scene.add(light2);
    const ambientLight = new THREE.AmbientLight(0x111111);
    scene.add(ambientLight);
    
    window.renderFrame = function(frameNum, fps) {
      const time = frameNum / fps;
      ring.rotation.z = time * 0.3;
      ring.rotation.x = Math.sin(time * 0.2) * 0.5;
      fireMaterial.uniforms.time.value = time;
      fireGroup.rotation.z = time * 0.3;
      
      renderer.setRenderTarget(renderTarget);
      renderer.render(scene, camera);
      
      renderer.setRenderTarget(null);
      postMaterial.uniforms.tDiffuse.value = renderTarget.texture;
      postMaterial.uniforms.time.value = time;
      renderer.render(postScene, postCamera);
      
      return renderer.domElement.toDataURL('image/png');
    };
    
    window.rendererReady = true;
    console.log("Renderer scripts initialized");
  </script>
</body>
</html>
`;

await page.setContent(html);
await page.waitForFunction(() => window.rendererReady);

for (let frame = startFrame; frame < endFrame; frame++) {
  const dataUrl = await page.evaluate((frameNum, fps) => {
    return window.renderFrame(frameNum, fps);
  }, frame, config.fps);

  const base64Data = dataUrl.replace(/^data:image\/png;base64,/, '');
  const buffer = Buffer.from(base64Data, 'base64');

  if (config.test) {
    const filename = `${config.output}_frame${frame}.png`;
    fs.writeFileSync(filename, buffer);
    console.error(`Wrote ${filename}`);
  } else {
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
