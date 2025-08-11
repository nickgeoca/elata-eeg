'use client';

import React, {useRef, useEffect} from 'react';
import {getChannelColor} from '../utils/colorUtils';
import {useDataBuffer} from '../hooks/useDataBuffer';
import {SampleChunk} from '../types/eeg';

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
  dataBuffer: ReturnType<typeof useDataBuffer<SampleChunk>>;
  width: number;
  height: number;
  uiVoltageScaleFactor: number;
}

export const EegRenderer = React.memo(function EegRenderer({
  isActive,
  config,
  dataBuffer,
  width,
  height,
  uiVoltageScaleFactor,
}: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // Render a blank canvas if the component is not active or if configuration is missing,
  // preventing crashes from accessing properties on undefined objects.
  if (!isActive || !config?.channels?.length) {
    return <canvas ref={canvasRef} className="w-full h-full" />;
  }

  const glRef     = useRef<WebGLRenderingContext | null>(null);
  const program   = useRef<WebGLProgram | null>(null);
  const location  = useRef<{
    pos: number; res: WebGLUniformLocation | null;
    sso: WebGLUniformLocation | null; col: WebGLUniformLocation | null;
  }>({pos:-1,res:null,sso:null,col:null});
  const vbos      = useRef<WebGLBuffer[]>([]);
  const cpuY      = useRef<Float32Array[]>([]);
  const rafId     = useRef<number>(0);
  const workerRef = useRef<Worker | null>(null);
  const ringBufferRef = useRef<SharedArrayBuffer | null>(null);

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

    /* VBO per channel, interleaved (x,y) */
    for (let ch = 0; ch < NCH; ch++) {
      const buf = gl.createBuffer()!;
      const arr = new Float32Array(NPTS * 2);
      for (let i = 0; i < NPTS; i++) arr[i * 2] = i; // x
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, arr, gl.DYNAMIC_DRAW);
      vbos.current.push(buf);
      cpuY.current.push(arr); // keep same reference, we’ll mutate y’s
    }

    // Initialize worker
    if (typeof SharedArrayBuffer !== 'undefined' && typeof Worker !== 'undefined') {
        const worker = new Worker(new URL('../workers/eegProcessor.worker.ts', import.meta.url));
        workerRef.current = worker;
        const buffer = new SharedArrayBuffer(NPTS * Float32Array.BYTES_PER_ELEMENT);
        ringBufferRef.current = buffer;
        worker.postMessage({ type: 'init', payload: { buffer } });
    } else {
        console.warn("SharedArrayBuffer or Worker is not available. Using a less performant fallback.");
    }

    return () => {
      if (workerRef.current) {
        workerRef.current.terminate();
      }
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
    if (location.current.res) gl.useProgram(program.current!),
      gl.uniform2f(location.current.res, w, h);
  }, [width, height]);

  /* ---------- render loop ---------- */
  useEffect(() => {
    if (!isActive || !glRef.current || !program.current) return;
    const gl = glRef.current;

    const draw = () => {
      // 1. ingest new EEG samples and shift data
      const chunks = dataBuffer.getAndClearData();
      if (chunks.length > 0 && workerRef.current && ringBufferRef.current) {
        chunks.forEach(chunk => {
          workerRef.current?.postMessage({ type: 'data', payload: chunk });
        });
      }

      if (ringBufferRef.current) {
        const ringBuffer = new Float32Array(ringBufferRef.current);
        for (let ch = 0; ch < NCH; ch++) {
          const ary = cpuY.current[ch];
          const data = ringBuffer.slice(); // In a real scenario, you'd read only new data
          for (let i = 0; i < NPTS; i++) {
            ary[i * 2 + 1] = data[i];
          }
          gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[ch]);
          gl.bufferSubData(gl.ARRAY_BUFFER, 0, ary);
        }
      } else {
        // Fallback rendering when SharedArrayBuffer is not available
        if (chunks.length > 0) {
          const batches: number[][] = Array.from({ length: NCH }, () => []);
          chunks.forEach(chk => {
            const samples = chk.samples;
            const numMetaChannels = chk.meta.channel_names.length;
            if (numMetaChannels === 0) return;

            if (NCH === 1) {
                const batch = batches[0];
                if (batch) {
                    for (let i = 0; i < samples.length; i++) {
                        batch.push(samples[i]);
                    }
                }
            } else {
                for (let i = 0; i < samples.length; i++) {
                    const channelIndex = i % numMetaChannels;
                    if (channelIndex < NCH) {
                        batches[channelIndex].push(samples[i]);
                    }
                }
            }
        });

        for (let ch = 0; ch < NCH; ch++) {
            if (!batches[ch] || !batches[ch].length) continue;

            const ary = cpuY.current[ch];
            const newVals = batches[ch];
            const numNew = newVals.length;

            if (numNew >= NPTS) {
                const latestVals = newVals.slice(-NPTS);
                for (let i = 0; i < NPTS; i++) {
                    ary[i * 2 + 1] = latestVals[i];
                }
            } else {
                const numExisting = NPTS - numNew;
                for (let i = 0; i < numExisting; i++) {
                    ary[i * 2 + 1] = ary[(i + numNew) * 2 + 1];
                }
                for (let i = 0; i < numNew; i++) {
                    ary[(numExisting + i) * 2 + 1] = newVals[i];
                }
            }

            gl.bindBuffer(gl.ARRAY_BUFFER, vbos.current[ch]);
            gl.bufferSubData(gl.ARRAY_BUFFER, 0, ary);
        }
      }
    }

      // 2. draw
      gl.clear(gl.COLOR_BUFFER_BIT);
      gl.useProgram(program.current!);
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

      rafId.current = requestAnimationFrame(draw);
    };
    draw();
    return () => cancelAnimationFrame(rafId.current);
  }, [isActive, dataBuffer, NCH, NPTS, YSCL]);

  return <canvas ref={canvasRef} className="w-full h-full" />;
});