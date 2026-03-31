import { c as computeScreenMapBounds } from "./graphics_utils-IETGgF1j.js";
const VERTEX_SHADER_SRC = `
        attribute vec2 a_position;
        attribute vec2 a_texCoord;
        varying vec2 v_texCoord;
        void main() {
            gl_Position = vec4(a_position, 0, 1);
            v_texCoord = a_texCoord;
        }
        `;
const FRAGMENT_SHADER_SRC = `
        precision mediump float;
        uniform sampler2D u_image;
        varying vec2 v_texCoord;
        void main() {
            gl_FragColor = texture2D(u_image, v_texCoord);
        }
        `;
function createShaders() {
  const fragmentShaderId = "fastled_FragmentShader";
  const vertexShaderId = "fastled_vertexShader";
  if (typeof document === "undefined") {
    return { vertex: VERTEX_SHADER_SRC, fragment: FRAGMENT_SHADER_SRC };
  }
  if (document.getElementById(fragmentShaderId) && document.getElementById(vertexShaderId)) {
    return { vertex: VERTEX_SHADER_SRC, fragment: FRAGMENT_SHADER_SRC };
  }
  const fragmentShader = document.createElement("script");
  const vertexShader = document.createElement("script");
  fragmentShader.id = fragmentShaderId;
  vertexShader.id = vertexShaderId;
  fragmentShader.type = "x-shader/x-fragment";
  vertexShader.type = "x-shader/x-vertex";
  fragmentShader.text = FRAGMENT_SHADER_SRC;
  vertexShader.text = VERTEX_SHADER_SRC;
  document.head.appendChild(fragmentShader);
  document.head.appendChild(vertexShader);
  return { vertex: VERTEX_SHADER_SRC, fragment: FRAGMENT_SHADER_SRC };
}
class GraphicsManager {
  /**
   * Creates a new GraphicsManager instance
   * @param {Object} graphicsArgs - Configuration options
   * @param {string} [graphicsArgs.canvasId] - ID of the canvas element to render to (main thread)
   * @param {HTMLCanvasElement|OffscreenCanvas} [graphicsArgs.canvas] - Canvas object directly (worker thread)
   * @param {Object} [graphicsArgs.threeJsModules] - Three.js modules (unused but kept for consistency)
   * @param {boolean} [graphicsArgs.usePixelatedRendering=true] - Whether to use pixelated rendering
   */
  constructor(graphicsArgs) {
    const { canvasId, canvas, usePixelatedRendering = true } = graphicsArgs;
    this.canvasId = canvas || canvasId;
    this.canvas = null;
    this.gl = null;
    this.program = null;
    this.positionBuffer = null;
    this.texCoordBuffer = null;
    this.texture = null;
    this.texData = null;
    this.screenMaps = {};
    this.args = { usePixelatedRendering };
    this.texWidth = 0;
    this.texHeight = 0;
    this.gridWidth = 0;
    this.gridHeight = 0;
    this._cachedGlobalBounds = null;
    this._boundsStale = true;
    this.initialize();
  }
  /**
   * Initializes the WebGL context and sets up rendering resources
   * @returns {boolean} True if initialization was successful
   */
  initialize() {
    const isOffscreenCanvas = typeof OffscreenCanvas !== "undefined" && this.canvasId instanceof OffscreenCanvas;
    const isHTMLCanvas = typeof HTMLCanvasElement !== "undefined" && this.canvasId instanceof HTMLCanvasElement;
    if (isOffscreenCanvas || isHTMLCanvas) {
      this.canvas = /** @type {HTMLCanvasElement|OffscreenCanvas} */
      this.canvasId;
    } else if (typeof this.canvasId === "string") {
      this.canvas = /** @type {HTMLCanvasElement|OffscreenCanvas} */
      document.getElementById(this.canvasId);
      if (!this.canvas) {
        console.error(`Canvas with id ${this.canvasId} not found`);
        return false;
      }
    } else {
      console.error("Invalid canvas parameter: must be string ID, HTMLCanvasElement, or OffscreenCanvas");
      return false;
    }
    this.gl = this.canvas.getContext("webgl");
    if (!this.gl) {
      console.error("WebGL not supported");
      return false;
    }
    return this.initWebGL();
  }
  /**
   * Updates the screenMaps data when it changes (event-driven)
   * Called directly from worker's handleScreenMapUpdate()
   * @param {Object} screenMapsData - Dictionary of screenmaps (stripId → screenmap object)
   */
  updateScreenMap(screenMapsData) {
    this.screenMaps = screenMapsData;
    this._boundsStale = true;
    for (const screenMap of Object.values(this.screenMaps)) {
      if (screenMap && screenMap.strips) {
        screenMap._cachedBounds = computeScreenMapBounds(screenMap);
      }
    }
    console.log("[GraphicsManager] ScreenMaps updated", {
      screenMapCount: Object.keys(screenMapsData || {}).length
    });
  }
  /**
   * Updates the display with new frame data from FastLED
   * @param {StripData[]} frameData - Array of LED strip data to render
   */
  updateDisplay(frameData) {
    if (!this.gl || !this.canvas) {
      console.warn("Graphics manager not properly initialized");
      return;
    }
    this.clearTexture();
    this.processFrameData(frameData);
    this.render();
  }
  /**
   * Clears the texture data buffer - optimized for recording performance
   */
  clearTexture() {
    if (this.texData) {
      this.texData.fill(0);
    }
  }
  /**
   * Processes frame data and updates texture
   * @param {StripData[] & {screenMap?: ScreenMapData}} frameData - Array of LED strip data to render with optional screenMap
   */
  processFrameData(frameData) {
    this.updateCanvas(frameData);
  }
  /**
   * Renders the current texture to the canvas
   */
  render() {
    if (!this.gl || !this.program) {
      return;
    }
    const canvasWidth = this.gl.canvas.width;
    const canvasHeight = this.gl.canvas.height;
    this.gl.viewport(0, 0, canvasWidth, canvasHeight);
    this.gl.clearColor(0, 0, 0, 1);
    this.gl.clear(this.gl.COLOR_BUFFER_BIT);
    this.gl.useProgram(this.program);
    const positionLocation = this.gl.getAttribLocation(this.program, "a_position");
    this.gl.enableVertexAttribArray(positionLocation);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.positionBuffer);
    this.gl.vertexAttribPointer(positionLocation, 2, this.gl.FLOAT, false, 0, 0);
    const texCoordLocation = this.gl.getAttribLocation(this.program, "a_texCoord");
    this.gl.enableVertexAttribArray(texCoordLocation);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.texCoordBuffer);
    this.gl.vertexAttribPointer(texCoordLocation, 2, this.gl.FLOAT, false, 0, 0);
    this.gl.drawArrays(this.gl.TRIANGLE_STRIP, 0, 4);
  }
  /**
   * Resets and cleans up WebGL resources
   * Disposes of buffers, textures, and programs to free memory
   */
  reset() {
    if (this.gl) {
      this.gl.deleteBuffer(this.positionBuffer);
      this.gl.deleteBuffer(this.texCoordBuffer);
      this.gl.deleteTexture(this.texture);
      this.gl.deleteProgram(this.program);
    }
    this.texWidth = 0;
    this.texHeight = 0;
    this.gl = null;
  }
  /**
   * Initializes the WebGL rendering context and resources
   * Sets up shaders, buffers, and textures for LED rendering
   * @returns {boolean} True if initialization was successful
   */
  initWebGL() {
    const shaders = createShaders();
    this.gl = this.canvas.getContext("webgl");
    if (!this.gl) {
      console.error("WebGL not supported");
      return false;
    }
    const vertexShader = this.createShader(
      this.gl.VERTEX_SHADER,
      shaders.vertex
    );
    const fragmentShader = this.createShader(
      this.gl.FRAGMENT_SHADER,
      shaders.fragment
    );
    this.program = this.createProgram(vertexShader, fragmentShader);
    this.positionBuffer = this.gl.createBuffer();
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.positionBuffer);
    this.gl.bufferData(
      this.gl.ARRAY_BUFFER,
      new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]),
      this.gl.STREAM_DRAW
    );
    this.texCoordBuffer = this.gl.createBuffer();
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.texCoordBuffer);
    this.gl.bufferData(
      this.gl.ARRAY_BUFFER,
      new Float32Array([0, 0, 1, 0, 0, 1, 1, 1]),
      this.gl.STREAM_DRAW
    );
    this.texture = this.gl.createTexture();
    this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_WRAP_S, this.gl.CLAMP_TO_EDGE);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_WRAP_T, this.gl.CLAMP_TO_EDGE);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MIN_FILTER, this.gl.NEAREST);
    this.gl.texParameteri(this.gl.TEXTURE_2D, this.gl.TEXTURE_MAG_FILTER, this.gl.NEAREST);
    return true;
  }
  /**
   * Creates and compiles a WebGL shader
   * @param {number} type - Shader type (VERTEX_SHADER or FRAGMENT_SHADER)
   * @param {string} source - GLSL shader source code
   * @returns {WebGLShader|null} Compiled shader or null on error
   */
  createShader(type, source) {
    const shader = this.gl.createShader(type);
    this.gl.shaderSource(shader, source);
    this.gl.compileShader(shader);
    if (!this.gl.getShaderParameter(shader, this.gl.COMPILE_STATUS)) {
      console.error("Shader compile error:", this.gl.getShaderInfoLog(shader));
      this.gl.deleteShader(shader);
      return null;
    }
    return shader;
  }
  /**
   * Creates and links a WebGL program from vertex and fragment shaders
   * @param {WebGLShader} vertexShader - Compiled vertex shader
   * @param {WebGLShader} fragmentShader - Compiled fragment shader
   * @returns {WebGLProgram|null} Linked program or null on error
   */
  createProgram(vertexShader, fragmentShader) {
    const program = this.gl.createProgram();
    this.gl.attachShader(program, vertexShader);
    this.gl.attachShader(program, fragmentShader);
    this.gl.linkProgram(program);
    if (!this.gl.getProgramParameter(program, this.gl.LINK_STATUS)) {
      console.error("Program link error:", this.gl.getProgramInfoLog(program));
      return null;
    }
    return program;
  }
  /**
   * Updates the canvas with new LED frame data
   * Processes strip data and renders LEDs to the WebGL texture
   * @param {StripData[] & {screenMap?: ScreenMapData}} frameData - Array of frame data containing LED strip information with optional screenMap
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
      return;
    }
    if (!this.gl) this.initWebGL();
    if (Object.keys(this.screenMaps).length > 0 && this.canvas) {
      if (this._boundsStale || !this._cachedGlobalBounds) {
        let globalMinX = Infinity, globalMinY = Infinity;
        let globalMaxX = -Infinity, globalMaxY = -Infinity;
        for (const screenMap of Object.values(this.screenMaps)) {
          const bounds = screenMap._cachedBounds || computeScreenMapBounds(screenMap);
          globalMinX = Math.min(globalMinX, bounds.absMin[0]);
          globalMinY = Math.min(globalMinY, bounds.absMin[1]);
          globalMaxX = Math.max(globalMaxX, bounds.absMax[0]);
          globalMaxY = Math.max(globalMaxY, bounds.absMax[1]);
        }
        this._cachedGlobalBounds = {
          minX: globalMinX,
          minY: globalMinY,
          gridWidth: globalMaxX - globalMinX + 1,
          gridHeight: globalMaxY - globalMinY + 1
        };
        this._boundsStale = false;
      }
      this.gridWidth = this._cachedGlobalBounds.gridWidth;
      this.gridHeight = this._cachedGlobalBounds.gridHeight;
      const MIN_CANVAS_DIM = 640;
      const maxDim = Math.max(this.gridWidth, this.gridHeight);
      const displayScale = maxDim > 0 ? Math.max(1, Math.ceil(MIN_CANVAS_DIM / maxDim)) : 1;
      const displayWidth = this.gridWidth * displayScale;
      const displayHeight = this.gridHeight * displayScale;
      if (this.canvas.width !== displayWidth || this.canvas.height !== displayHeight) {
        try {
          this.canvas.width = displayWidth;
          this.canvas.height = displayHeight;
        } catch (error) {
          console.log("Canvas resize skipped - canvas is controlled by worker");
          return;
        }
        if (this.gl) {
          this.gl.viewport(0, 0, displayWidth, displayHeight);
        }
      }
    }
    const canvasWidth = this.gl.canvas.width;
    const canvasHeight = this.gl.canvas.height;
    const texBaseWidth = this.gridWidth || canvasWidth;
    const texBaseHeight = this.gridHeight || canvasHeight;
    const newTexWidth = 2 ** Math.ceil(Math.log2(texBaseWidth));
    const newTexHeight = 2 ** Math.ceil(Math.log2(texBaseHeight));
    if (this.texWidth !== newTexWidth || this.texHeight !== newTexHeight) {
      this.texWidth = newTexWidth;
      this.texHeight = newTexHeight;
      this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture);
      this.gl.texImage2D(
        this.gl.TEXTURE_2D,
        0,
        this.gl.RGB,
        this.texWidth,
        this.texHeight,
        0,
        this.gl.RGB,
        this.gl.UNSIGNED_BYTE,
        null
      );
      this.texData = new Uint8Array(this.texWidth * this.texHeight * 3);
      console.log(`WebGL texture reallocated: ${this.texWidth}x${this.texHeight} (${(this.texData.length / 1024 / 1024).toFixed(1)}MB)`);
    }
    if (Object.keys(this.screenMaps).length === 0) {
      console.warn("No screenMaps found, skipping update");
      return;
    }
    this.texData.fill(0);
    for (let i = 0; i < frameData.length; i++) {
      const strip = frameData[i];
      if (!strip) {
        console.warn("Null strip encountered, skipping");
        continue;
      }
      const data = strip.pixel_data;
      if (!data || typeof data.length !== "number") {
        console.warn("Invalid pixel data for strip:", strip);
        continue;
      }
      const { strip_id } = strip;
      const screenMap = this.screenMaps[strip_id];
      if (!screenMap) {
        console.warn(`No screenMap found for strip ${strip_id}`);
        continue;
      }
      if (!(strip_id in screenMap.strips)) {
        console.warn(`Strip ${strip_id} not found in its screenMap`);
        continue;
      }
      const stripData = screenMap.strips[strip_id];
      const pixelCount = data.length / 3;
      const { map } = stripData;
      const bounds = screenMap._cachedBounds || computeScreenMapBounds(screenMap);
      const min_x = bounds.absMin[0];
      const min_y = bounds.absMin[1];
      const x_array = map.x;
      const y_array = map.y;
      const len = Math.min(x_array.length, y_array.length);
      for (let i2 = 0; i2 < pixelCount; i2++) {
        if (i2 >= len) {
          console.warn(
            `Strip ${strip_id}: Pixel ${i2} is outside the screen map ${map.length}, skipping update`
          );
          continue;
        }
        let x = x_array[i2];
        let y = y_array[i2];
        x -= min_x;
        y -= min_y;
        x = x | 0;
        y = y | 0;
        if (x < 0 || x >= this.gridWidth || y < 0 || y >= this.gridHeight) {
          console.warn(
            `Strip ${strip_id}: Pixel ${i2} is outside the canvas at ${x}, ${y}, skipping update`
          );
          continue;
        }
        const diameter = stripData.diameter || 1;
        const radius = Math.floor(diameter / 2);
        for (let dy = -radius; dy <= radius; dy++) {
          for (let dx = -radius; dx <= radius; dx++) {
            const px = x + dx;
            const py = y + dy;
            if (px >= 0 && px < this.gridWidth && py >= 0 && py < this.gridHeight) {
              const srcIndex = i2 * 3;
              const destIndex = (py * this.texWidth + px) * 3;
              const r = data[srcIndex] & 255;
              const g = data[srcIndex + 1] & 255;
              const b = data[srcIndex + 2] & 255;
              this.texData[destIndex] = r;
              this.texData[destIndex + 1] = g;
              this.texData[destIndex + 2] = b;
            }
          }
        }
      }
    }
    this.gl.bindTexture(this.gl.TEXTURE_2D, this.texture);
    this.gl.texSubImage2D(
      this.gl.TEXTURE_2D,
      0,
      0,
      0,
      this.texWidth,
      this.texHeight,
      this.gl.RGB,
      this.gl.UNSIGNED_BYTE,
      this.texData
    );
    this.gl.viewport(0, 0, canvasWidth, canvasHeight);
    this.gl.clearColor(0, 0, 0, 1);
    this.gl.clear(this.gl.COLOR_BUFFER_BIT);
    this.gl.useProgram(this.program);
    const positionLocation = this.gl.getAttribLocation(this.program, "a_position");
    this.gl.enableVertexAttribArray(positionLocation);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.positionBuffer);
    this.gl.vertexAttribPointer(positionLocation, 2, this.gl.FLOAT, false, 0, 0);
    const texCoordLocation = this.gl.getAttribLocation(this.program, "a_texCoord");
    this.gl.enableVertexAttribArray(texCoordLocation);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.texCoordBuffer);
    this.gl.vertexAttribPointer(texCoordLocation, 2, this.gl.FLOAT, false, 0, 0);
    const gridW = this.gridWidth || canvasWidth;
    const gridH = this.gridHeight || canvasHeight;
    const texCoords = new Float32Array([
      0,
      0,
      gridW / this.texWidth,
      0,
      0,
      gridH / this.texHeight,
      gridW / this.texWidth,
      gridH / this.texHeight
    ]);
    this.gl.bindBuffer(this.gl.ARRAY_BUFFER, this.texCoordBuffer);
    this.gl.bufferData(this.gl.ARRAY_BUFFER, texCoords, this.gl.STREAM_DRAW);
    this.gl.drawArrays(this.gl.TRIANGLE_STRIP, 0, 4);
  }
}
export {
  GraphicsManager
};
//# sourceMappingURL=graphics_manager-Bwn1yiaQ.js.map
