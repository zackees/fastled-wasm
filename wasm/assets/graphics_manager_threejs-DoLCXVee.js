import { c as computeScreenMapBounds, i as isDenseGrid } from "./graphics_utils-IETGgF1j.js";
function makePositionCalculators(screenMap, screenWidth, screenHeight) {
  const bounds = screenMap && screenMap.absMin && screenMap.absMax ? { absMin: screenMap.absMin, absMax: screenMap.absMax } : computeScreenMapBounds(screenMap);
  const width = bounds.absMax[0] - bounds.absMin[0];
  const height = bounds.absMax[1] - bounds.absMin[1];
  if (Number.isNaN(width) || Number.isNaN(height) || width === 0 && height === 0) {
    console.error("Invalid screenmap bounds detected:");
    console.error(`  absMin: [${bounds.absMin[0]}, ${bounds.absMin[1]}]`);
    console.error(`  absMax: [${bounds.absMax[0]}, ${bounds.absMax[1]}]`);
    console.error(`  width: ${width}, height: ${height}`);
    console.error("This indicates the screenmap was not properly set up.");
    console.error("Make sure to call .setScreenMap() on your LED controller in setup().");
  }
  return {
    /**
     * Calculates X position in 3D space from screen coordinates
     * @param {number} x - Screen X coordinate
     * @returns {number} 3D X position centered around origin
     */
    calcXPosition: (x) => {
      if (width === 0) return 0;
      return (x - bounds.absMin[0]) / width * screenWidth - screenWidth / 2;
    },
    /**
     * Calculates Y position in 3D space from screen coordinates
     * @param {number} y - Screen Y coordinate
     * @returns {number} 3D Y position centered around origin
     */
    calcYPosition: (y) => {
      if (height === 0) return 0;
      const negY = (y - bounds.absMin[1]) / height * screenHeight - screenHeight / 2;
      return negY;
    }
  };
}
class GraphicsManagerThreeJS {
  /**
   * Creates a new GraphicsManagerThreeJS instance
   * @param {Object} graphicsArgs - Configuration options
   * @param {string} [graphicsArgs.canvasId] - ID of the canvas element to render to (main thread)
   * @param {HTMLCanvasElement|OffscreenCanvas} [graphicsArgs.canvas] - Canvas object directly (worker thread)
   * @param {Object} graphicsArgs.threeJsModules - Three.js modules and dependencies
   */
  constructor(graphicsArgs) {
    const { canvasId, canvas, threeJsModules } = graphicsArgs;
    this.canvasId = canvas || canvasId;
    this.threeJsModules = threeJsModules;
    this.SEGMENTS = 16;
    this.LED_SCALE = 1;
    this.SCREEN_WIDTH = 0;
    this.SCREEN_HEIGHT = 0;
    this.bloom_stength = 1;
    this.bloom_radius = 16;
    this.current_brightness = 0;
    this.target_brightness = 0;
    this.iris_response_speed = 0.05;
    this.base_bloom_strength = 16;
    this.max_bloom_strength = 20;
    this.min_bloom_strength = 0.5;
    this.auto_bloom_enabled = true;
    this.leds = [];
    this.mergedMeshes = [];
    this.scene = null;
    this.camera = null;
    this.renderer = null;
    this.composer = null;
    this.previousTotalLeds = 0;
    this.outside_bounds_warning_count = 0;
    this.useMergedGeometry = true;
    this.canvas = null;
    this.screenMaps = {};
    this.needsRebuild = false;
  }
  /**
   * Cleans up and resets the rendering environment
   * Disposes of all Three.js objects and clears scene
   */
  reset() {
    if (this.leds) {
      this.leds.forEach((led) => {
        if (!led._isMerged) {
          led.geometry.dispose();
          led.material.dispose();
          this.scene?.remove(led);
        }
      });
    }
    this.leds = [];
    if (this.mergedMeshes) {
      this.mergedMeshes.forEach((mesh) => {
        if (mesh.geometry) mesh.geometry.dispose();
        if (mesh.material) mesh.material.dispose();
        this.scene?.remove(mesh);
      });
    }
    this.mergedMeshes = [];
    if (this.composer) {
      this.composer.dispose();
    }
    if (this.scene) {
      while (this.scene.children.length > 0) {
        this.scene.remove(this.scene.children[0]);
      }
    }
    if (this.renderer) {
      const isOffscreenCanvas = typeof OffscreenCanvas !== "undefined" && this.canvas instanceof OffscreenCanvas;
      this.renderer.setSize(this.SCREEN_WIDTH, this.SCREEN_HEIGHT, !isOffscreenCanvas);
    }
  }
  /**
   * Updates the screenMaps data when it changes (event-driven)
   * Called directly from worker's handleScreenMapUpdate()
   * Triggers scene rebuild on next frame if scene already exists
   * @param {Object} screenMapsData - Dictionary of screenmaps (stripId → screenmap object)
   */
  updateScreenMap(screenMapsData) {
    this.screenMaps = screenMapsData;
    if (this.scene) {
      this.needsRebuild = true;
      console.log("[GraphicsManagerThreeJS] ScreenMaps updated, scene will rebuild");
    } else {
      console.log("[GraphicsManagerThreeJS] ScreenMaps received before scene initialization");
    }
  }
  /**
   * Initializes the Three.js rendering environment
   * Sets up scene, camera, renderer, and post-processing pipeline
   * @param {Object} frameData - The frame data containing screen mapping information
   */
  initThreeJS(frameData) {
    if (!this.canvas) {
      const isOffscreenCanvas = typeof OffscreenCanvas !== "undefined" && this.canvasId instanceof OffscreenCanvas;
      const isHTMLCanvas = typeof HTMLCanvasElement !== "undefined" && this.canvasId instanceof HTMLCanvasElement;
      if (isOffscreenCanvas || isHTMLCanvas) {
        this.canvas = /** @type {HTMLCanvasElement|OffscreenCanvas} */
        this.canvasId;
      } else if (typeof this.canvasId === "string") {
        const canvasElement = document.getElementById(this.canvasId);
        if (!canvasElement) {
          console.warn("initThreeJS: Canvas element not found");
          return;
        }
        this.canvas = /** @type {HTMLCanvasElement|OffscreenCanvas} */
        canvasElement;
      } else {
        console.error("Invalid canvas parameter: must be string ID, HTMLCanvasElement, or OffscreenCanvas", {
          canvasIdType: typeof this.canvasId,
          canvasIdValue: this.canvasId
        });
        return;
      }
    } else {
      console.log("initThreeJS: Reusing existing canvas reference");
    }
    if (!this.canvas) {
      console.error("initThreeJS: Canvas resolution failed - cannot initialize Three.js");
      return;
    }
    this._setupCanvasAndDimensions(frameData);
    this._setupScene();
    this._setupRenderer();
    this._setupRenderPasses(frameData);
    if (this.camera) {
      this.camera.updateProjectionMatrix();
    }
  }
  /**
   * Sets up canvas dimensions and display properties
   * @private
   * @param {Object} _frameData - Frame data (unused, kept for API compatibility)
   */
  _setupCanvasAndDimensions(_frameData) {
    const RESOLUTION_BOOST = 2;
    const MAX_WIDTH = 640;
    if (!this.canvas) {
      console.error("_setupCanvasAndDimensions: canvas is undefined or null", {
        hasCanvasId: !!this.canvasId,
        canvasIdType: typeof this.canvasId,
        canvasIdValue: this.canvasId
      });
      throw new Error("Canvas reference is undefined in _setupCanvasAndDimensions");
    }
    let globalMinX = Infinity, globalMinY = Infinity;
    let globalMaxX = -Infinity, globalMaxY = -Infinity;
    for (const screenMap of Object.values(this.screenMaps)) {
      const bounds = computeScreenMapBounds(screenMap);
      globalMinX = Math.min(globalMinX, bounds.absMin[0]);
      globalMinY = Math.min(globalMinY, bounds.absMin[1]);
      globalMaxX = Math.max(globalMaxX, bounds.absMax[0]);
      globalMaxY = Math.max(globalMaxY, bounds.absMax[1]);
    }
    let screenMapWidth = globalMaxX - globalMinX;
    let screenMapHeight = globalMaxY - globalMinY;
    const MIN_DIMENSION = 1;
    if (screenMapWidth <= 0 || !Number.isFinite(screenMapWidth)) {
      console.warn("_setupCanvasAndDimensions: screenMapWidth is <= 0 or invalid, using MIN_DIMENSION", {
        screenMapWidth,
        screenMapsCount: Object.keys(this.screenMaps).length
      });
      screenMapWidth = MIN_DIMENSION;
    }
    if (screenMapHeight <= 0 || !Number.isFinite(screenMapHeight)) {
      console.warn("_setupCanvasAndDimensions: screenMapHeight is <= 0 or invalid, using MIN_DIMENSION", {
        screenMapHeight,
        screenMapsCount: Object.keys(this.screenMaps).length
      });
      screenMapHeight = MIN_DIMENSION;
    }
    const targetWidth = MAX_WIDTH;
    const aspectRatio = screenMapWidth / screenMapHeight;
    const targetHeight = Math.round(targetWidth / aspectRatio);
    if (!Number.isFinite(targetHeight) || targetHeight <= 0) {
      console.error("_setupCanvasAndDimensions: Invalid targetHeight calculated", {
        targetHeight,
        aspectRatio,
        screenMapWidth,
        screenMapHeight
      });
      throw new Error(`Invalid canvas height: ${targetHeight}. Check screen mapping dimensions.`);
    }
    this.SCREEN_WIDTH = targetWidth * RESOLUTION_BOOST;
    this.SCREEN_HEIGHT = targetHeight * RESOLUTION_BOOST;
    this.canvas.width = targetWidth * RESOLUTION_BOOST;
    this.canvas.height = targetHeight * RESOLUTION_BOOST;
    if (typeof window !== "undefined" && !(this.canvas instanceof OffscreenCanvas) && this.canvas.style) {
      this.canvas.style.width = `${targetWidth}px`;
      this.canvas.style.height = `${targetHeight}px`;
      this.canvas.style.maxWidth = `${targetWidth}px`;
      this.canvas.style.maxHeight = `${targetHeight}px`;
    }
    this.screenBounds = {
      width: screenMapWidth,
      height: screenMapHeight,
      minX: globalMinX,
      minY: globalMinY,
      maxX: globalMaxX,
      maxY: globalMaxY
    };
  }
  /**
   * Sets up the Three.js scene and camera
   * @private
   */
  _setupScene() {
    const { THREE } = this.threeJsModules;
    this.scene = new THREE.Scene();
    this._setupCamera();
  }
  /**
   * Sets up the camera with proper positioning and projection
   * @private
   */
  _setupCamera() {
    const { THREE } = this.threeJsModules;
    const NEAR_PLANE = 0.1;
    const FAR_PLANE = 5e3;
    const MARGIN = 1.05;
    const halfWidth = this.SCREEN_WIDTH / 2;
    const halfHeight = this.SCREEN_HEIGHT / 2;
    this.camera = new THREE.OrthographicCamera(
      -halfWidth * MARGIN,
      // left
      halfWidth * MARGIN,
      // right
      halfHeight * MARGIN,
      // top
      -halfHeight * MARGIN,
      // bottom
      NEAR_PLANE,
      FAR_PLANE
    );
    this.camera.position.set(0, 0, 1e3);
    this.camera.zoom = 1;
    this.camera.updateProjectionMatrix();
  }
  /**
   * Sets up the WebGL renderer
   * @private
   */
  _setupRenderer() {
    const { THREE } = this.threeJsModules;
    this.renderer = new THREE.WebGLRenderer({
      canvas: this.canvas,
      // Use stored canvas reference
      antialias: true
    });
    const isOffscreenCanvas = typeof OffscreenCanvas !== "undefined" && this.canvas instanceof OffscreenCanvas;
    this.renderer.setSize(this.SCREEN_WIDTH, this.SCREEN_HEIGHT, !isOffscreenCanvas);
  }
  /**
   * Sets up render passes including bloom effect
   * @private
   */
  _setupRenderPasses(frameData) {
    const {
      THREE,
      EffectComposer,
      RenderPass,
      UnrealBloomPass
    } = this.threeJsModules;
    const renderScene = new RenderPass(this.scene, this.camera);
    this.composer = new EffectComposer(this.renderer);
    this.composer.addPass(renderScene);
    const { isDenseScreenMap } = this.createGrid(frameData);
    if (!isDenseScreenMap) {
      this.bloom_stength = 16;
      this.bloom_radius = 1;
    } else {
      this.bloom_stength = 0;
      this.bloom_radius = 0;
    }
    if (this.bloom_stength > 0 || this.bloom_radius > 0) {
      const bloomPass = new UnrealBloomPass(
        new THREE.Vector2(this.SCREEN_WIDTH, this.SCREEN_HEIGHT),
        this.bloom_stength,
        this.bloom_radius,
        0
        // threshold
      );
      this.composer.addPass(bloomPass);
    }
  }
  /**
   * Creates the LED grid based on frame data
   * @param {Object} frameData - The frame data containing screen mapping information
   * @returns {Object} - Object containing isDenseScreenMap flag
   */
  createGrid(frameData) {
    this._clearExistingLeds();
    const ledPositions = this._collectLedPositions(frameData);
    let globalMinX = Infinity, globalMinY = Infinity;
    let globalMaxX = -Infinity, globalMaxY = -Infinity;
    for (const screenMap of Object.values(this.screenMaps)) {
      const bounds = computeScreenMapBounds(screenMap);
      globalMinX = Math.min(globalMinX, bounds.absMin[0]);
      globalMinY = Math.min(globalMinY, bounds.absMin[1]);
      globalMaxX = Math.max(globalMaxX, bounds.absMax[0]);
      globalMaxY = Math.max(globalMaxY, bounds.absMax[1]);
    }
    const width = globalMaxX - globalMinX;
    const height = globalMaxY - globalMinY;
    const compositeScreenMap = {
      absMin: [globalMinX, globalMinY],
      absMax: [globalMaxX, globalMaxY]
    };
    const { calcXPosition, calcYPosition } = makePositionCalculators(
      compositeScreenMap,
      this.SCREEN_WIDTH,
      this.SCREEN_HEIGHT
    );
    let isDenseScreenMap = true;
    for (const screenMap of Object.values(this.screenMaps)) {
      if (!isDenseGrid(screenMap)) {
        isDenseScreenMap = false;
        break;
      }
    }
    const { defaultDotSize, normalizedScale } = this._calculateDotSizes(
      frameData,
      ledPositions,
      width,
      height,
      calcXPosition,
      isDenseScreenMap
    );
    this._createLedObjects(
      frameData,
      calcXPosition,
      calcYPosition,
      isDenseScreenMap,
      defaultDotSize,
      normalizedScale
    );
    return { isDenseScreenMap };
  }
  /**
   * Clears existing LED objects
   * @private
   */
  _clearExistingLeds() {
    this.leds.forEach((led) => {
      led.geometry.dispose();
      led.material.dispose();
      this.scene?.remove(led);
    });
    this.leds = [];
  }
  /**
   * Collects all LED positions from frame data
   * @private
   */
  _collectLedPositions(frameData) {
    const ledPositions = [];
    frameData.forEach((strip) => {
      const stripId = strip.strip_id;
      const screenMap = this.screenMaps[stripId];
      if (!screenMap) {
        console.warn(`[GraphicsManagerThreeJS] No screenMap found for strip ${stripId}`);
        return;
      }
      if (!(stripId in screenMap.strips)) {
        console.warn(`[GraphicsManagerThreeJS] Strip ${stripId} not found in its screenMap`);
        return;
      }
      const stripMap = screenMap.strips[stripId];
      const x_array = stripMap.map.x;
      const y_array = stripMap.map.y;
      for (let i = 0; i < x_array.length; i++) {
        ledPositions.push([x_array[i], y_array[i]]);
      }
    });
    return ledPositions;
  }
  /**
   * Calculates appropriate dot sizes for LEDs
   * @private
   */
  _calculateDotSizes(frameData, ledPositions, width, height, calcXPosition, isDenseScreenMap) {
    const screenArea = width * height;
    let pixelDensityDefault;
    if (isDenseScreenMap) {
      console.log("Pixel density is close to 1, assuming grid or strip");
      pixelDensityDefault = Math.abs(calcXPosition(0) - calcXPosition(1));
    }
    const defaultDotSizeScale = Math.max(
      4,
      Math.sqrt(screenArea / (ledPositions.length * Math.PI)) * 0.4
    );
    const stripDotSizes = [];
    for (const screenMap of Object.values(this.screenMaps)) {
      for (const strip of Object.values(screenMap.strips)) {
        stripDotSizes.push(strip.diameter);
      }
    }
    const avgPointDiameter = stripDotSizes.length > 0 ? stripDotSizes.reduce((a, b) => a + b, 0) / stripDotSizes.length : 0.5;
    let defaultDotSize = defaultDotSizeScale * avgPointDiameter;
    if (pixelDensityDefault) {
      defaultDotSize = pixelDensityDefault;
    }
    const normalizedScale = this.SCREEN_WIDTH / width;
    return { defaultDotSize, normalizedScale };
  }
  /**
   * Creates LED objects for each pixel in the frame data
   * @private
   */
  _createLedObjects(frameData, calcXPosition, calcYPosition, isDenseScreenMap, defaultDotSize, normalizedScale) {
    const { THREE } = this.threeJsModules;
    const { BufferGeometryUtils } = this.threeJsModules;
    const canMergeGeometries = this.useMergedGeometry && BufferGeometryUtils && true;
    if (!canMergeGeometries) {
      console.log("BufferGeometryUtils not available, falling back to individual LEDs");
    } else {
      console.log("Using merged geometries for better performance");
    }
    let circleGeometry;
    let planeGeometry;
    if (!isDenseScreenMap) {
      circleGeometry = new THREE.CircleGeometry(1, this.SEGMENTS);
    } else {
      planeGeometry = new THREE.PlaneGeometry(1, 1);
    }
    const allGeometries = [];
    const allLedData = [];
    frameData.forEach((strip) => {
      const stripId = strip.strip_id;
      const screenMap = this.screenMaps[stripId];
      if (!screenMap) {
        console.warn(`[GraphicsManagerThreeJS] No screenMap found for strip ${stripId} in _createLedObjects`);
        return;
      }
      if (!(stripId in screenMap.strips)) {
        console.warn(`[GraphicsManagerThreeJS] Strip ${stripId} not found in its screenMap in _createLedObjects`);
        return;
      }
      const stripData = screenMap.strips[stripId];
      let stripDiameter = null;
      if (stripData.diameter) {
        stripDiameter = stripData.diameter * normalizedScale;
      } else {
        stripDiameter = defaultDotSize;
      }
      const x_array = stripData.map.x;
      const y_array = stripData.map.y;
      for (let i = 0; i < x_array.length; i++) {
        const x = calcXPosition(x_array[i]);
        const y = calcYPosition(y_array[i]);
        const z = 500;
        if (Number.isNaN(x) || Number.isNaN(y)) {
          console.error(`Invalid LED coordinates detected for strip ${stripId}, LED ${i}:`);
          console.error(`  x=${x}, y=${y} (raw: x_array[${i}]=${x_array[i]}, y_array[${i}]=${y_array[i]})`);
          console.error(`  This usually means you created a ScreenMap but forgot to call .setScreenMap() on the controller.`);
          console.error(`  Example: FastLED.addLeds<...>(leds, NUM_LEDS).setScreenMap(yourScreenMap);`);
          throw new Error(`Invalid LED coordinates for strip ${stripId}, LED ${i}. Did you forget to call .setScreenMap()?`);
        }
        if (!canMergeGeometries) {
          let geometry;
          if (isDenseScreenMap) {
            const w = stripDiameter * this.LED_SCALE;
            const h = stripDiameter * this.LED_SCALE;
            geometry = new THREE.PlaneGeometry(w, h);
          } else {
            geometry = new THREE.CircleGeometry(
              stripDiameter * this.LED_SCALE,
              this.SEGMENTS
            );
          }
          const material = new THREE.MeshBasicMaterial({ color: 0 });
          const led = new THREE.Mesh(geometry, material);
          led.position.set(x, y, z);
          this.scene.add(led);
          this.leds.push(led);
        } else {
          let instanceGeometry;
          if (isDenseScreenMap) {
            instanceGeometry = planeGeometry.clone();
            instanceGeometry.scale(
              stripDiameter * this.LED_SCALE,
              stripDiameter * this.LED_SCALE,
              1
            );
          } else {
            instanceGeometry = circleGeometry.clone();
            instanceGeometry.scale(
              stripDiameter * this.LED_SCALE,
              stripDiameter * this.LED_SCALE,
              1
            );
          }
          instanceGeometry.translate(x, y, z);
          allGeometries.push(instanceGeometry);
          allLedData.push({ x, y, z });
        }
      }
    });
    if (canMergeGeometries && allGeometries.length > 0) {
      try {
        const mergedGeometry = BufferGeometryUtils.mergeGeometries(allGeometries);
        const material = new THREE.MeshBasicMaterial({
          color: 16777215,
          vertexColors: true
        });
        const colorCount = mergedGeometry.attributes.position.count;
        const colorArray = new Float32Array(colorCount * 3);
        mergedGeometry.setAttribute("color", new THREE.BufferAttribute(colorArray, 3));
        const mesh = new THREE.Mesh(mergedGeometry, material);
        this.scene.add(mesh);
        this.mergedMeshes.push(mesh);
        for (let i = 0; i < allLedData.length; i++) {
          const pos = allLedData[i];
          const dummyObj = {
            material: { color: new THREE.Color(0, 0, 0) },
            position: new THREE.Vector3(pos.x, pos.y, pos.z),
            _isMerged: true,
            _mergedIndex: i,
            _parentMesh: mesh
          };
          this.leds.push(dummyObj);
        }
      } catch (e) {
        console.log(BufferGeometryUtils);
        console.error("Failed to merge geometries:", e);
        for (let i = 0; i < allGeometries.length; i++) {
          const pos = allLedData[i];
          const material = new THREE.MeshBasicMaterial({ color: 0 });
          const geometry = allGeometries[i].clone();
          geometry.translate(-pos.x, -pos.y, -pos.z);
          const led = new THREE.Mesh(geometry, material);
          led.position.set(pos.x, pos.y, pos.z);
          this.scene.add(led);
          this.leds.push(led);
        }
      }
      if (circleGeometry) circleGeometry.dispose();
      if (planeGeometry) planeGeometry.dispose();
      allGeometries.forEach((g) => g.dispose());
    }
  }
  /**
   * Updates the canvas with new frame data
   * @param {Object} frameData - The frame data containing LED colors and positions
   */
  updateCanvas(frameData) {
    if (!frameData) {
      console.warn("Received null frame data, skipping update");
      return;
    }
    if (!Array.isArray(frameData)) {
      console.warn("Received non-array frame data:", frameData);
      return;
    }
    if (frameData.length === 0) {
      console.warn("Received empty frame data, skipping update");
    }
    this._checkAndInitializeScene(frameData);
    const positionMap = this._collectLedColorData(frameData);
    if (this.auto_bloom_enabled) {
      this._updateAutoBrightness(positionMap);
    }
    if (Object.keys(this.screenMaps).length > 0) {
      this._updateLedVisuals(positionMap);
    } else {
      console.warn("No screenMaps available for LED visual updates");
    }
    this.composer.render();
  }
  /**
   * Updates auto-brightness tracking and adjusts bloom effect
   * @private
   * @param {Map} positionMap - Map of LED positions to color data
   */
  _updateAutoBrightness(positionMap) {
    if (!positionMap || positionMap.size === 0) {
      return;
    }
    let totalBrightness = 0;
    let count = 0;
    for (const [, ledData] of positionMap) {
      totalBrightness += ledData.brightness;
      count++;
    }
    this.target_brightness = count > 0 ? totalBrightness / count : 0;
    const delta = this.target_brightness - this.current_brightness;
    this.current_brightness += delta * this.iris_response_speed;
    const densityFactor = count / Math.max(this.leds.length, 1);
    const invertedBrightness = 1 - this.current_brightness;
    const bloomStrength = this.min_bloom_strength + (this.max_bloom_strength - this.min_bloom_strength) * invertedBrightness * densityFactor;
    this._updateBloomStrength(bloomStrength);
  }
  /**
   * Updates the bloom pass strength dynamically
   * @private
   * @param {number} strength - New bloom strength value
   */
  _updateBloomStrength(strength) {
    if (!this.composer || !this.composer.passes) {
      return;
    }
    for (const pass of this.composer.passes) {
      if (pass.constructor.name === "UnrealBloomPass") {
        pass.strength = strength;
        this.bloom_stength = strength;
        break;
      }
    }
  }
  /**
   * Checks if scene needs initialization and handles it
   * @private
   */
  _checkAndInitializeScene(frameData) {
    if (!frameData || !Array.isArray(frameData)) {
      console.warn("Invalid frame data in _checkAndInitializeScene:", frameData);
      return;
    }
    const totalPixels = frameData.reduce(
      (acc, strip) => {
        if (!strip || !strip.pixel_data || typeof strip.pixel_data.length !== "number") {
          console.warn("Invalid strip data:", strip);
          return acc;
        }
        return acc + strip.pixel_data.length / 3;
      },
      0
    );
    if (!this.scene || totalPixels !== this.previousTotalLeds || this.needsRebuild) {
      if (this.scene) {
        this.reset();
      }
      this.initThreeJS(frameData);
      this.previousTotalLeds = totalPixels;
      this.needsRebuild = false;
    }
  }
  /**
   * Collects LED color data from frame data
   * @private
   * @returns {Map} - Map of LED positions to color data
   */
  _collectLedColorData(frameData) {
    const dataArray = Array.isArray(frameData) ? frameData : frameData.data || [];
    if (Object.keys(this.screenMaps).length === 0) {
      console.warn("No screenMaps available");
      return /* @__PURE__ */ new Map();
    }
    const positionMap = /* @__PURE__ */ new Map();
    const WARNING_COUNT = 10;
    dataArray.forEach((strip) => {
      if (!strip) {
        console.warn("Null strip encountered, skipping");
        return;
      }
      const { strip_id } = strip;
      const screenMap = this.screenMaps[strip_id];
      if (!screenMap) {
        console.warn(`No screenMap found for strip ${strip_id}`);
        return;
      }
      if (!screenMap.strips || !(strip_id in screenMap.strips)) {
        console.warn(`Strip ${strip_id} not found in its screenMap`);
        return;
      }
      const stripData = screenMap.strips[strip_id];
      if (!stripData || !stripData.map) {
        console.warn(`Invalid strip data for strip ID ${strip_id}:`, stripData);
        return;
      }
      const { map } = stripData;
      const data = strip.pixel_data;
      if (!data || typeof data.length !== "number") {
        console.warn(`Invalid pixel data for strip ID ${strip_id}:`, data);
        return;
      }
      const pixelCount = data.length / 3;
      const x_array = stripData.map.x;
      const y_array = stripData.map.y;
      if (!x_array || !y_array || typeof x_array.length !== "number" || typeof y_array.length !== "number") {
        console.warn(`Invalid coordinate arrays for strip ID ${strip_id}:`, { x_array, y_array });
        return;
      }
      const length = Math.min(x_array.length, y_array.length);
      for (let j = 0; j < pixelCount; j++) {
        if (j >= length) {
          this._handleOutOfBoundsPixel(strip_id, j, map.length, WARNING_COUNT);
          continue;
        }
        const x = x_array[j];
        const y = y_array[j];
        const posKey = `${x},${y}`;
        const srcIndex = j * 3;
        const r = (data[srcIndex] & 255) / 255;
        const g = (data[srcIndex + 1] & 255) / 255;
        const b = (data[srcIndex + 2] & 255) / 255;
        const brightness = (r + g + b) / 3;
        if (!positionMap.has(posKey) || positionMap.get(posKey).brightness < brightness) {
          positionMap.set(posKey, {
            x,
            y,
            r,
            g,
            b,
            brightness
          });
        }
      }
    });
    return positionMap;
  }
  /**
   * Handles warning for pixels outside the screen map bounds
   * @private
   */
  _handleOutOfBoundsPixel(strip_id, j, mapLength, WARNING_COUNT) {
    this.outside_bounds_warning_count++;
    if (this.outside_bounds_warning_count < WARNING_COUNT) {
      console.warn(
        `Strip ${strip_id}: Pixel ${j} is outside the screen map ${mapLength}, skipping update`
      );
      if (this.outside_bounds_warning_count === WARNING_COUNT) {
        console.warn("Suppressing further warnings about pixels outside the screen map");
      }
    }
    console.warn(
      `Strip ${strip_id}: Pixel ${j} is outside the screen map ${mapLength}, skipping update`
    );
  }
  /**
   * Updates LED visuals based on position map data
   * @private
   */
  _updateLedVisuals(positionMap) {
    const { THREE } = this.threeJsModules;
    const min_x = this.screenBounds.minX;
    const min_y = this.screenBounds.minY;
    const { width } = this.screenBounds;
    const { height } = this.screenBounds;
    const mergedMeshUpdates = /* @__PURE__ */ new Map();
    let ledIndex = 0;
    for (const [, ledData] of positionMap) {
      if (ledIndex >= this.leds.length) break;
      const led = this.leds[ledIndex];
      const x = ledData.x - min_x;
      const y = ledData.y - min_y;
      const normalizedX = x / width * this.SCREEN_WIDTH - this.SCREEN_WIDTH / 2;
      const normalizedY = y / height * this.SCREEN_HEIGHT - this.SCREEN_HEIGHT / 2;
      const z = this._calculateDepthEffect();
      if (led._isMerged) {
        led.material.color.setRGB(ledData.r, ledData.g, ledData.b);
        led.position.set(normalizedX, normalizedY, z);
        if (led._parentMesh) {
          if (!mergedMeshUpdates.has(led._parentMesh)) {
            mergedMeshUpdates.set(led._parentMesh, []);
          }
          mergedMeshUpdates.get(led._parentMesh).push({
            index: led._mergedIndex,
            color: new THREE.Color(ledData.r, ledData.g, ledData.b)
          });
        }
      } else {
        led.position.set(normalizedX, normalizedY, z);
        led.material.color.setRGB(ledData.r, ledData.g, ledData.b);
      }
      ledIndex++;
    }
    this._updateMergedMeshes(mergedMeshUpdates);
    this._clearUnusedLeds(ledIndex);
  }
  /**
   * Updates merged meshes with new colors
   * @private
   */
  _updateMergedMeshes(mergedMeshUpdates) {
    const { THREE } = this.threeJsModules;
    for (const [mesh, updates] of mergedMeshUpdates.entries()) {
      if (!mesh.geometry || !mesh.material) continue;
      if (!mesh.geometry.attributes.color) {
        const { count } = mesh.geometry.attributes.position;
        const colorArray = new Float32Array(count * 3);
        mesh.geometry.setAttribute("color", new THREE.BufferAttribute(colorArray, 3));
        mesh.material.vertexColors = true;
      }
      const colorAttribute = mesh.geometry.attributes.color;
      updates.forEach((update) => {
        const { index, color } = update;
        const verticesPerInstance = mesh.geometry.attributes.position.count / this.leds.length;
        for (let v = 0; v < verticesPerInstance; v++) {
          const i = (index * verticesPerInstance + v) * 3;
          colorAttribute.array[i] = color.r;
          colorAttribute.array[i + 1] = color.g;
          colorAttribute.array[i + 2] = color.b;
        }
      });
      colorAttribute.needsUpdate = true;
    }
  }
  /**
   * Calculates a depth effect based on distance from center
   * @private
   */
  _calculateDepthEffect() {
    return 0;
  }
  /**
   * Clears unused LEDs by setting them to black and moving offscreen
   * @private
   */
  _clearUnusedLeds(startIndex) {
    const mergedMeshUpdates = /* @__PURE__ */ new Map();
    const { THREE } = this.threeJsModules;
    for (let i = startIndex; i < this.leds.length; i++) {
      const led = this.leds[i];
      led.material.color.setRGB(0, 0, 0);
      if (!led._isMerged) {
        led.position.set(-1e3, -1e3, 0);
      } else {
        led.position.set(-1e3, -1e3, 0);
        if (led._parentMesh) {
          if (!mergedMeshUpdates.has(led._parentMesh)) {
            mergedMeshUpdates.set(led._parentMesh, []);
          }
          mergedMeshUpdates.get(led._parentMesh).push({
            index: led._mergedIndex,
            color: new THREE.Color(0, 0, 0)
          });
        }
      }
    }
    this._updateMergedMeshes(mergedMeshUpdates);
  }
}
export {
  GraphicsManagerThreeJS
};
//# sourceMappingURL=graphics_manager_threejs-DoLCXVee.js.map
