function computeScreenMapBounds(screenMap) {
  if (!screenMap || typeof screenMap !== "object" || !screenMap.strips) {
    console.warn("computeScreenMapBounds: Invalid screenMap structure");
    return { absMin: [0, 0], absMax: [0, 0] };
  }
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let hasData = false;
  for (const stripId in screenMap.strips) {
    if (!Object.prototype.hasOwnProperty.call(screenMap.strips, stripId)) continue;
    const strip = screenMap.strips[stripId];
    if (!strip || !strip.map || !strip.map.x || !strip.map.y) {
      continue;
    }
    const xArray = strip.map.x;
    const yArray = strip.map.y;
    const len = Math.min(xArray.length, yArray.length);
    for (let i = 0; i < len; i++) {
      const x = xArray[i];
      const y = yArray[i];
      if (typeof x === "number" && typeof y === "number" && !Number.isNaN(x) && !Number.isNaN(y)) {
        minX = Math.min(minX, x);
        maxX = Math.max(maxX, x);
        minY = Math.min(minY, y);
        maxY = Math.max(maxY, y);
        hasData = true;
      }
    }
  }
  if (!hasData) {
    return { absMin: [0, 0], absMax: [0, 0] };
  }
  return {
    absMin: [minX, minY],
    absMax: [maxX, maxY]
  };
}
function isDenseGrid(screenMap) {
  if (!screenMap || typeof screenMap !== "object") {
    console.warn("isDenseGrid: screenMap is not a valid object");
    return false;
  }
  if (!screenMap.strips || typeof screenMap.strips !== "object") {
    console.warn("isDenseGrid: screenMap.strips is not a valid object");
    return false;
  }
  let allPixelDensitiesUndefined2 = true;
  for (const stripId in screenMap.strips) {
    if (!Object.prototype.hasOwnProperty.call(screenMap.strips, stripId)) continue;
    const strip = screenMap.strips[stripId];
    if (!strip || typeof strip !== "object") {
      console.warn(`isDenseGrid: Invalid strip data for strip ${stripId}`);
      continue;
    }
    allPixelDensitiesUndefined2 = allPixelDensitiesUndefined2 && strip.diameter === void 0;
    if (!allPixelDensitiesUndefined2) {
      break;
    }
  }
  if (!allPixelDensitiesUndefined2) {
    return false;
  }
  let totalPixels = 0;
  for (const stripId in screenMap.strips) {
    if (!Object.prototype.hasOwnProperty.call(screenMap.strips, stripId)) continue;
    const stripMap = screenMap.strips[stripId];
    if (stripMap && stripMap.map && stripMap.map.x && stripMap.map.y) {
      const len = Math.min(stripMap.map.x.length, stripMap.map.y.length);
      totalPixels += len;
    }
  }
  const bounds = computeScreenMapBounds(screenMap);
  const width = 1 + (bounds.absMax[0] - bounds.absMin[0]);
  const height = 1 + (bounds.absMax[1] - bounds.absMin[1]);
  const screenArea = width * height;
  if (screenArea <= 0) {
    console.warn("isDenseGrid: Invalid screen area calculation");
    return false;
  }
  const pixelDensity = totalPixels / screenArea;
  return pixelDensity > 0.9 && pixelDensity < 1.1;
}
export {
  computeScreenMapBounds as c,
  isDenseGrid as i
};
//# sourceMappingURL=graphics_utils-IETGgF1j.js.map
