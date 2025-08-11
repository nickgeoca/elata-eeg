'use client';

import React, {useRef, useEffect} from 'react';
import { getChannelColor } from '../utils/colorUtils';
import { useEegData } from '../context/EegDataContext';
import { SampleChunk } from '../types/eeg';

const VS = `
attribute vec2 a_xy;                // (sampleIndex, value)
uniform vec2  u_res;                // canvas resolution
uniform vec3  u_scrollScaleOffset;  // (xScale, yScale, yOffset)
void main() {
  vec2 pos = vec2(
    a_xy.x * u_scrollScaleOffset.x,             // scale time
    a_xy.y * u_scrollScaleOffset.y +            // scale EEG amp
      u_scrollScaleOffset.z);                   // move to channel band
  vec2 clip = (pos / u_res) * 2.0 - 1.0;
  gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}
`;

const FS = `
precision mediump float;
uniform vec4 u_color;
void main() { gl_FragColor = u_color; }
`;

interface Props {
  isActive: boolean;
  config: {channels: number[]; samplesPerLine?: number; ampScale?: number};
  width: number;
  height: number;
  uiVoltageScaleFactor: number;
}

export const EegRenderer = React.memo(function EegRenderer({
  isActive,
  config,
  width,
  height,
  uiVoltageScaleFactor,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { drainIncoming, getRawSamples, subscribeRaw } = useEegData();

  if (!isActive || !config?.channels?.length) {
    return <canvas ref={canvasRef} className="w-full h-full" />;
  }

  const glRef     = useRef<WebGLRenderingContext | null>(null);
  const program   = useRef<WebGLProgram | null>(null);
  const location  = useRef<{
    pos: number; res: WebGLUniformLocation | null;
    sso: WebGLUniformLocation | null; col: WebGLUniformLocation | null;
  }>({pos:-1,res:null,sso:null,col:null});
  const vbos = useRef<WebGLBuffer[]>([]);
  const cpuY = useRef<Float32Array[]>([]);
  const rafId = useRef<number>(0);
  const lastProcessedTimestamp = useRef<number>(0);
  const scratch = useRef<Float32Array[]>([]);
  const lastRenderTime = useRef<number>(0);

  const NCH   = config.channels.length;
  const NPTS  = config.samplesPerLine ?? 1024;
  const YSCL  = 100000.0*(uiVoltageScaleFactor ?? 0.01);

  /* ---------- init (once) ---------- */
  useEffect(() => {
    if (!isActive || !canvasRef.current) return;

    const gl = canvasRef.current.getContext('webgl');
    if (!gl) return console.error('WebGL ctx failed');
    glRef.current = gl;

    // build program
    const compile = (type: number, src: string) => {
      const s = gl.createShader(type)!; gl.shaderSource(s, src); gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS))
        throw new Error(gl.getShaderInfoLog(s) ?? '');
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, compile(gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, FS));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS))
      throw new Error(gl.getProgramInfoLog(prog) ?? '');
    program.current = prog;

    // locations
    location.current.pos = gl.getAttribLocation(prog, 'a_xy');
    location.current.res = gl.getUniformLocation(prog, 'u_res');
    location.current.sso = gl.getUniformLocation(prog, 'u_scrollScaleOffset');
    location.current.col = gl.getUniformLocation(prog, 'u_color');

    gl.disable(gl.DEPTH_TEST);
    gl.clearColor(0, 0, 0, 0); // Set clear color once

    /* VBO per channel, interleaved (x,y) */
    for (let ch = 0; ch < NCH; ch++) {
      const buf = gl.createBuffer()!;
      const arr = new Float32Array(NPTS * 2);
      for (let i = 0; i < NPTS; i++) arr[i * 2] = i; // x
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, arr, gl.DYNAMIC_DRAW);
      vbos.current.push(buf);
      cpuY.current.push(arr); // keep same reference, we’ll mutate y’s
      scratch.current[ch] = new Float32Array(NPTS); // Pre-allocate scratch buffer
    }

    return () => {
      cancelAnimationFrame(rafId.current);
      const gl = glRef.current;
      const prog = program.current;
      if (gl && prog) {
        vbos.current.forEach(b => gl.deleteBuffer(b));
        gl.deleteProgram(prog);
      }
      vbos.current = [];
      cpuY.current = [];
      program.current = null;
      glRef.current = null;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isActive, NCH, NPTS]);

  /* ---------- resize --------- */
  useEffect(() => {
    const gl = glRef.current; if (!gl) return;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.round(width  * dpr);
    const h = Math.round(height * dpr);
    const cvs = gl.canvas as HTMLCanvasElement;
    if (cvs.width !== w || cvs.height !== h) {
      cvs.width = w; cvs.height = h; gl.viewport(0,0,w,h);
    }
    gl.useProgram(program.current!);
    if (location.current.res) {
      gl.uniform2f(location.current.res, w, h);
    }
  }, [width, height]);

  /* ---------- render loop ---------- */
  useEffect(() => {
    if (!isActive || !glRef.current || !program.current) return;
    const gl = glRef.current;

    const targetHz = 30; // Less aggressive throttling
    const frameMs = 1000 / targetHz;

    const draw = (currentTime: number) => {
      // Throttle to 30Hz with accumulator pattern
      if (currentTime - lastRenderTime.current >= frameMs) {
        // Drain incoming data with a small time budget
        drainIncoming(2); // Smaller budget to prevent blocking

        const allChunks = getRawSamples();
        const newChunks = allChunks.filter(c => c.timestamp > lastProcessedTimestamp.current);

        if (newChunks.length > 0) {
          lastProcessedTimestamp.current = newChunks[newChunks.length - 1].timestamp;

          for (let ch = 0; ch < NCH; ch++) {
            const ary = cpuY.current[ch];
            const scratchY = scratch.current[ch];
            let offset = 0;

            newChunks.forEach(chunk => {
              const samples = chunk.samples;
              const numMetaChannels = chunk.meta.channel_names.length;
              if (numMetaChannels === 0 || offset >= scratchY.length) return;

              if (NCH === 1) {
                const remainingSpace = scratchY.length - offset;
                const numToCopy = Math.min(samples.length, remainingSpace);
                scratchY.set(samples.subarray(0, numToCopy), offset);
                offset += numToCopy;
              } else {
                for (let i = ch; i < samples.length && offset < scratchY.length; i += numMetaChannels) {
                  scratchY[offset++] = samples[i];
                }
              }
            });

            const numNew = offset;
            if (numNew === 0) continue;

            if (numNew >= NPTS) {
              const latestVals = scratchY.subarray(numNew - NPTS, numNew);
              for (let i = 0; i < NPTS; i++) {
                ary[i * 2 + 1] = latestVals[i];
              }
            } else {
              // Shift existing data left
              ary.copyWithin(1, (numNew * 2) + 1);
              // Set new data at the end
              for (let i = 0; i < numNew; i++) {
                ary[(NPTS - numNew + i) * 2 + 1] = scratchY[i];
              }
            }
            gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[ch]);
            gl.bufferSubData(gl.ARRAY_BUFFER, 0, ary);
          }
        }

        // 2. draw
        gl.clear(gl.COLOR_BUFFER_BIT);
        gl.enableVertexAttribArray(location.current.pos);
        gl.vertexAttribPointer(location.current.pos, 2, gl.FLOAT, false, 0, 0);

        const rowH = gl.canvas.height / NCH;
        for (let ch = 0; ch < NCH; ch++) {
          gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[ch]);
          const yOff = rowH * (ch + 0.5);
          gl.uniform3f(location.current.sso!, gl.canvas.width / NPTS, YSCL, yOff);
          const [r,g,b] = getChannelColor(ch);
          gl.uniform4f(location.current.col!, r,g,b,1);
          gl.drawArrays(gl.LINE_STRIP, 0, NPTS);
        }

        lastRenderTime.current = currentTime;
      }

      rafId.current = requestAnimationFrame(draw);
    };
    rafId.current = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(rafId.current);
  }, [isActive, NCH, NPTS, YSCL, drainIncoming, getRawSamples, subscribeRaw]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
});