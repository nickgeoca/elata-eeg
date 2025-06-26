'use client';

import React, { useEffect, useRef, useCallback, useImperativeHandle } from 'react';
import { getChannelColor } from '../utils/colorUtils';

interface EegCircularRendererProps {
  canvasRef: React.RefObject<HTMLCanvasElement>;
  config: any;
  numPoints: number; // Total points per channel (sampling_rate * display_seconds)
  targetFps?: number;
  containerWidth: number;
  containerHeight: number;
}

export interface EegCircularRendererRef {
  addNewSample(channelIndex: number, value: number): void;
}

export const EegCircularRenderer = React.memo(React.forwardRef<EegCircularRendererRef, EegCircularRendererProps>(function EegCircularRenderer({
  canvasRef,
  config,
  numPoints,
  targetFps,
  containerWidth,
  containerHeight
}, ref) {
  const glRef = useRef<WebGL2RenderingContext | null>(null);
  const programRef = useRef<WebGLProgram | null>(null);
  const vbosRef = useRef<WebGLBuffer[]>([]);
  const animationFrameRef = useRef<number | null>(null);
  const headPositionRef = useRef<number>(0);
  const lastRenderTimeRef = useRef<number>(0);
  const isInitializedRef = useRef<boolean>(false);
  
  const numChannels = config?.channels?.length ?? 8;

  // Initialize WebGL2 context and resources
  const initWebGL = useCallback(() => {
    if (!canvasRef.current) return false;
    
    const canvas = canvasRef.current;
    const gl = canvas.getContext('webgl2');
    if (!gl) {
      console.error('WebGL2 not supported');
      return false;
    }
    
    glRef.current = gl;
    
    // Compile shaders
    const vertexShader = gl.createShader(gl.VERTEX_SHADER)!;
    gl.shaderSource(vertexShader, `#version 300 es
      in float a_eegValue;
      uniform float u_headPosition;
      uniform float u_totalPoints;
      uniform float u_channelYOffset;
      uniform float u_amplitudeScale;
      
      void main() {
        float relativeIndex = float(gl_VertexID) - u_headPosition;
        if (relativeIndex < 0.0) {
          relativeIndex += u_totalPoints;
        }
        float x = relativeIndex / u_totalPoints;
        float x_ndc = (x * 2.0) - 1.0;
        float y_ndc = u_channelYOffset + (a_eegValue * u_amplitudeScale);
        gl_Position = vec4(x_ndc, y_ndc, 0.0, 1.0);
      }
    `);
    gl.compileShader(vertexShader);
    
    const fragmentShader = gl.createShader(gl.FRAGMENT_SHADER)!;
    gl.shaderSource(fragmentShader, `#version 300 es
      precision mediump float;
      uniform vec4 u_lineColor;
      out vec4 outColor;
      
      void main() {
        outColor = u_lineColor;
      }
    `);
    gl.compileShader(fragmentShader);
    
    // Create program
    const program = gl.createProgram()!;
    gl.attachShader(program, vertexShader);
    gl.attachShader(program, fragmentShader);
    gl.linkProgram(program);
    programRef.current = program;
    
    // Create VBOs for each channel
    vbosRef.current = [];
    for (let i = 0; i < numChannels; i++) {
      const vbo = gl.createBuffer()!;
      gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(numPoints), gl.DYNAMIC_DRAW);
      vbosRef.current.push(vbo);
    }
    
    return true;
  }, [canvasRef, numChannels, numPoints]);

  // Render loop
  const renderLoop = useCallback(() => {
    animationFrameRef.current = requestAnimationFrame(renderLoop);
    
    const gl = glRef.current;
    const program = programRef.current;
    if (!gl || !program || numChannels === 0) return;
    
    const now = performance.now();
    
    // FPS throttling
    if (targetFps && targetFps > 0) {
      const frameInterval = 1000 / targetFps;
      const elapsed = now - lastRenderTimeRef.current;
      if (elapsed < frameInterval) return;
      lastRenderTimeRef.current = now - (elapsed % frameInterval);
    } else {
      lastRenderTimeRef.current = now;
    }
    
    gl.useProgram(program);
    
    // Set common uniforms
    const headPosLoc = gl.getUniformLocation(program, 'u_headPosition');
    const totalPointsLoc = gl.getUniformLocation(program, 'u_totalPoints');
    const amplitudeScaleLoc = gl.getUniformLocation(program, 'u_amplitudeScale');
    
    gl.uniform1f(headPosLoc, headPositionRef.current);
    gl.uniform1f(totalPointsLoc, numPoints);
    gl.uniform1f(amplitudeScaleLoc, 0.5); // Adjust based on config
    
    // Render each channel
    for (let ch = 0; ch < numChannels; ch++) {
      gl.bindBuffer(gl.ARRAY_BUFFER, vbosRef.current[ch]);
      
      // Set vertex attribute
      const positionLoc = gl.getAttribLocation(program, 'a_eegValue');
      gl.enableVertexAttribArray(positionLoc);
      gl.vertexAttribPointer(positionLoc, 1, gl.FLOAT, false, 0, 0);
      
      // Set channel-specific uniforms
      const yOffsetLoc = gl.getUniformLocation(program, 'u_channelYOffset');
      const colorLoc = gl.getUniformLocation(program, 'u_lineColor');
      
      const yOffset = -1.0 + (ch / numChannels) * 2.0;
      gl.uniform1f(yOffsetLoc, yOffset);
      
      const color = getChannelColor(ch);
      gl.uniform4f(colorLoc, color[0], color[1], color[2], 1.0);
      
      // Draw the line
      gl.drawArrays(gl.LINE_STRIP, 0, numPoints);
    }
  }, [numChannels, numPoints, targetFps]);

  // Initialization effect
  useEffect(() => {
    if (isInitializedRef.current || !canvasRef.current || 
        containerWidth <= 0 || containerHeight <= 0) return;
    
    // Size canvas
    const canvas = canvasRef.current;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(containerWidth * dpr);
    canvas.height = Math.round(containerHeight * dpr);
    canvas.style.width = `${containerWidth}px`;
    canvas.style.height = `${containerHeight}px`;
    
    // Initialize WebGL
    if (initWebGL()) {
      isInitializedRef.current = true;
      animationFrameRef.current = requestAnimationFrame(renderLoop);
    }
    
    return () => {
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
      }
      glRef.current = null;
      programRef.current = null;
      vbosRef.current = [];
      isInitializedRef.current = false;
    };
  }, [canvasRef, containerWidth, containerHeight, initWebGL, renderLoop]);

  // Resize effect
  useEffect(() => {
    if (!canvasRef.current || !glRef.current || containerWidth <= 0 || containerHeight <= 0) return;
    
    const canvas = canvasRef.current;
    const dpr = window.devicePixelRatio || 1;
    const physicalWidth = Math.round(containerWidth * dpr);
    const physicalHeight = Math.round(containerHeight * dpr);
    
    if (canvas.width !== physicalWidth || canvas.height !== physicalHeight) {
      canvas.width = physicalWidth;
      canvas.height = physicalHeight;
      canvas.style.width = `${containerWidth}px`;
      canvas.style.height = `${containerHeight}px`;
      glRef.current.viewport(0, 0, physicalWidth, physicalHeight);
    }
  }, [canvasRef, containerWidth, containerHeight]);

  // Data update handler
  useImperativeHandle(ref, () => ({
    addNewSample(channelIndex: number, value: number) {
      if (!glRef.current || channelIndex >= vbosRef.current.length) return;
      
      const gl = glRef.current;
      gl.bindBuffer(gl.ARRAY_BUFFER, vbosRef.current[channelIndex]);
      
      // Update single point at current head position
      const offset = headPositionRef.current * 4; // Float32 = 4 bytes
      gl.bufferSubData(gl.ARRAY_BUFFER, offset, new Float32Array([value]));

      // Move head position to the next spot for the next sample
      // We only increment the head once per batch of samples (e.g., once for all channels)
      if (channelIndex === numChannels - 1) {
        headPositionRef.current = (headPositionRef.current + 1) % numPoints;
      }
    }
  }), [numChannels, numPoints]);

  return null;
}));