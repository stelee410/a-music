import React from 'react';
import {AbsoluteFill, useCurrentFrame} from 'remotion';
import {ThreeCanvas} from '@remotion/three';
import {Stage} from './Stage';
import {FPS, DURATION, CRASH, env, clamp, section} from './song';

/** 《拟心 Simulated Heart》. No text anywhere in the picture (AUTHORING.md §7.4). */
export const Video = () => {
  const frame = useCurrentFrame();
  const t = frame / FPS;
  const s = section(t);
  const chorus = ['emerge1', 'emerge2', 'heart'].includes(s.id);
  const flash = chorus ? 0.22 * env(CRASH, t, 0.12) : 0;
  const fade = clamp(Math.min(t / 0.5, (DURATION - t) / 2.2));
  return (
    <AbsoluteFill style={{backgroundColor: '#000'}}>
      <ThreeCanvas width={1280} height={720} flat camera={{fov: 30, position: [0, 0, 3.732], near: 0.1, far: 60}}>
        <Stage />
      </ThreeCanvas>
      <AbsoluteFill style={{background: 'radial-gradient(ellipse at center, rgba(0,0,0,0) 52%, rgba(0,0,0,0.62) 100%)'}} />
      <AbsoluteFill style={{backgroundColor: '#fff', opacity: flash, mixBlendMode: 'screen'}} />
      <AbsoluteFill style={{backgroundColor: '#000', opacity: 1 - fade}} />
    </AbsoluteFill>
  );
};
