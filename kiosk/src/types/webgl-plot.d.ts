// kiosk/src/types/webgl-plot.d.ts
/**
 * Manual type declarations for 'webgl-plot' (@next version) to work around
 * issues with package.json "exports" preventing automatic type resolution.
 * Declarations based on observed API in examples and .d.ts files.
 * REVERTING to multi-line API based on app/roll.js example.
 */
declare module 'webgl-plot' {

  // Based on ColorRGBA.d.ts
  export class ColorRGBA {
    r: number;
    g: number;
    b: number;
    a: number;
    constructor(r: number, g: number, b: number, a: number);
  }

  // Based on WbglLineRoll.d.ts and app/roll.js example (MULTI-LINE version)
  export class WebglLineRoll {
    // Constructor signature from example (wglp, width, numLines)
    constructor(wglp: WebglPlot, width: number, numLines: number);

    // Method from example (accepts array of arrays of numbers)
    addPoints(yArrays: number[][]): void;

    // Method from example (commented out due to runtime errors, but signature kept)
    setLineColor(color: ColorRGBA, lineIndex: number): void;

    // Method from example
    draw(): void;

    // NOTE: addPoint(yValue: number) does NOT exist in this multi-line version.
  }

  // Based on webglplot.d.ts - Declaring used parts
  export class WebglPlot {
    gl: WebGL2RenderingContext;
    gScaleX: number;
    gScaleY: number;

    // Constructor signature used
    constructor(canvas: HTMLCanvasElement, options?: {
        antialias?: boolean;
        transparent?: boolean;
        powerPerformance?: "default" | "high-performance" | "low-power";
        deSync?: boolean;
        preserveDrawing?: boolean;
        debug?: boolean;
    });
    // Method used
    update(): void; // Keep for now
    // Method used
    clear(): void;
  }
}