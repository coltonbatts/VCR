import * as THREE from 'three';
import * as RAPIER from '@dimforge/rapier3d-compat';

class RapierThreeApp {
    private scene: THREE.Scene;
    private camera: THREE.PerspectiveCamera;
    private renderer: THREE.WebGLRenderer;
    private world: RAPIER.World;
    private cubeBody: RAPIER.RigidBody;
    private cubeMesh: THREE.Mesh;

    // Determinism settings
    private readonly fixedDeltaTime = 1 / 60;
    private accumulator = 0;
    private lastTime = 0;

    constructor() {
        this.scene = new THREE.Scene();
        this.camera = new THREE.PerspectiveCamera(75, window.innerWidth / window.innerHeight, 0.1, 1000);
        this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    }

    async init() {
        console.log("🚀 Starting Hardened Rapier + Three.js App");

        try {
            await RAPIER.init();
            console.log("✅ Rapier initialized. Version:", RAPIER.version());

            this.setupPhysics();
            this.setupGraphics();
            this.setupLights();
            this.createEnvironment();
            this.createObjects();

            this.camera.position.set(0, 5, 12);
            this.camera.lookAt(0, 2, 0);

            window.addEventListener('resize', () => this.onResize());

            this.lastTime = performance.now();
            this.animate();

        } catch (error) {
            this.handleError(error);
        }
    }

    private setupPhysics() {
        const gravity = { x: 0.0, y: -9.81, z: 0.0 };
        this.world = new RAPIER.World(gravity);
    }

    private setupGraphics() {
        this.renderer.setSize(window.innerWidth, window.innerHeight);
        this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
        this.renderer.setClearColor(0x000000, 0);
        document.body.appendChild(this.renderer.domElement);
    }

    private setupLights() {
        const ambientLight = new THREE.AmbientLight(0xffffff, 0.4);
        this.scene.add(ambientLight);

        const mainLight = new THREE.PointLight(0x00ffff, 200, 50);
        mainLight.position.set(5, 10, 5);
        this.scene.add(mainLight);

        const rimLight = new THREE.PointLight(0xff00ff, 150, 50);
        rimLight.position.set(-5, 5, -5);
        this.scene.add(rimLight);
    }

    private createEnvironment() {
        // Metallic Ground with Grid
        const groundSize = 40;
        const groundGeometry = new THREE.PlaneGeometry(groundSize, groundSize);
        const groundMaterial = new THREE.MeshStandardMaterial({
            color: 0x0a0b1e,
            metalness: 0.8,
            roughness: 0.2
        });
        const groundMesh = new THREE.Mesh(groundGeometry, groundMaterial);
        groundMesh.rotation.x = -Math.PI / 2;
        this.scene.add(groundMesh);

        // Grid Helper for "VCR" aesthetic
        const grid = new THREE.GridHelper(groundSize, 40, 0x00ffff, 0x1a1a2e);
        grid.position.y = 0.01;
        this.scene.add(grid);

        // Physics for ground
        const groundBodyDesc = RAPIER.RigidBodyDesc.fixed();
        const groundBody = this.world.createRigidBody(groundBodyDesc);
        const groundColliderDesc = RAPIER.ColliderDesc.cuboid(groundSize / 2, 0.1, groundSize / 2);
        this.world.createCollider(groundColliderDesc, groundBody);
    }

    private createObjects() {
        const cubeSize = 1.0;

        // Physics Cube
        const cubeBodyDesc = RAPIER.RigidBodyDesc.dynamic()
            .setTranslation(0, 10, 0)
            .setAngularDamping(0.5)
            .setCanSleep(false);
        this.cubeBody = this.world.createRigidBody(cubeBodyDesc);

        const cubeColliderDesc = RAPIER.ColliderDesc.cuboid(cubeSize / 2, cubeSize / 2, cubeSize / 2)
            .setRestitution(0.5);
        this.world.createCollider(cubeColliderDesc, this.cubeBody);

        // Graphics Cube
        const cubeGeometry = new THREE.BoxGeometry(cubeSize, cubeSize, cubeSize);
        const cubeMaterial = new THREE.MeshStandardMaterial({
            color: 0xff00ff,
            metalness: 0.6,
            roughness: 0.1,
            emissive: 0xff00ff,
            emissiveIntensity: 0.2
        });
        this.cubeMesh = new THREE.Mesh(cubeGeometry, cubeMaterial);
        this.scene.add(this.cubeMesh);
    }

    private onResize() {
        this.camera.aspect = window.innerWidth / window.innerHeight;
        this.camera.updateProjectionMatrix();
        this.renderer.setSize(window.innerWidth, window.innerHeight);
    }

    private animate() {
        requestAnimationFrame(() => this.animate());

        const currentTime = performance.now();
        const deltaTime = (currentTime - this.lastTime) / 1000;
        this.lastTime = currentTime;

        // Fixed Timestep Accumulator Loop
        this.accumulator += deltaTime;
        while (this.accumulator >= this.fixedDeltaTime) {
            this.world.step();
            this.accumulator -= this.fixedDeltaTime;
        }

        // Sync visual mesh with physics body
        const translation = this.cubeBody.translation();
        const rotation = this.cubeBody.rotation();

        this.cubeMesh.position.set(translation.x, translation.y, translation.z);
        this.cubeMesh.quaternion.set(rotation.x, rotation.y, rotation.z, rotation.w);

        this.renderer.render(this.scene, this.camera);
    }

    private handleError(err: any) {
        console.error("❌ Initialization failed:", err);
        const errorDiv = document.createElement('div');
        errorDiv.style.color = '#ff00ff';
        errorDiv.style.fontFamily = 'monospace';
        errorDiv.style.position = 'absolute';
        errorDiv.style.top = '20px';
        errorDiv.style.left = '20px';
        errorDiv.style.background = 'rgba(0,0,0,0.8)';
        errorDiv.style.padding = '10px';
        errorDiv.style.border = '1px solid #ff00ff';
        errorDiv.innerText = "FATAL ERROR: " + (err instanceof Error ? err.message : String(err));
        document.body.appendChild(errorDiv);
    }
}

new RapierThreeApp().init();
