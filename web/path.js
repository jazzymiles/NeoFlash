/* Canvas geometry for NeoFlash vector paths. No flattening or bounding-box hit approximation. */
(function (global) {
  'use strict';

  function createPath(commands) {
    const path = new Path2D();
    for (const command of commands) {
      switch (command.op) {
        case 'move_to': path.moveTo(command.x, command.y); break;
        case 'line_to': path.lineTo(command.x, command.y); break;
        case 'quadratic_to': path.quadraticCurveTo(command.cx, command.cy, command.x, command.y); break;
        case 'cubic_to': path.bezierCurveTo(command.cx1, command.cy1, command.cx2, command.cy2, command.x, command.y); break;
        case 'close': path.closePath(); break;
        default: throw new Error(`Unsupported path operation: ${command.op}`);
      }
    }
    return path;
  }

  class PathCache {
    constructor({ maxKeyBytes = 8 * 1024 * 1024, maxEntries = 2048, pathFactory = createPath } = {}) {
      if (!Number.isSafeInteger(maxKeyBytes) || maxKeyBytes < 0 || !Number.isSafeInteger(maxEntries) || maxEntries < 0) {
        throw new Error('Path cache budgets must be non-negative safe integers.');
      }
      this.paths = new Map();
      this.keyBytes = 0;
      this.maxKeyBytes = maxKeyBytes;
      this.maxEntries = maxEntries;
      this.pathFactory = pathFactory;
    }
    get(commands) {
      const key = JSON.stringify(commands);
      const cached = this.paths.get(key);
      if (cached) return cached.path;
      const path = this.pathFactory(commands);
      // Count UTF-16 bytes conservatively. Native Path2D allocation is browser-dependent;
      // bounding geometry keys plus entry count also bounds the retained input complexity.
      const keyBytes = key.length * 2;
      if (keyBytes > this.maxKeyBytes || this.maxEntries <= 0) return path;
      while (this.paths.size >= this.maxEntries || this.keyBytes + keyBytes > this.maxKeyBytes) {
        const oldestKey = this.paths.keys().next().value;
        this.keyBytes -= this.paths.get(oldestKey).keyBytes;
        this.paths.delete(oldestKey);
      }
      this.paths.set(key, { path, keyBytes });
      this.keyBytes += keyBytes;
      return path;
    }
  }

  function contains(context, kind, x, y, path = createPath(kind.commands)) {
    context.save();
    try {
      // The caller already applied the inverse composed instance transform to the point.
      // DPR and the last rendered object's transform must not affect local geometry tests.
      context.setTransform(1, 0, 0, 1, 0, 0);
      if (kind.fill[3] > 0 && context.isPointInPath(path, x, y, kind.fill_rule || 'nonzero')) return true;
      if (kind.stroke[3] <= 0 || kind.stroke_width <= 0) return false;
      context.lineWidth = kind.stroke_width;
      context.lineCap = 'round';
      context.lineJoin = 'round';
      return context.isPointInStroke(path, x, y);
    } finally {
      context.restore();
    }
  }

  const api = { createPath, PathCache, contains };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else global.NeoFlashPaths = api;
})(globalThis);
