import React from 'react';
import {Composition} from 'remotion';
import {Video} from './Video';
import {FPS, DURATION} from './song';

export const Root = () => (
  <Composition id="Video" component={Video} width={1280} height={720} fps={FPS} durationInFrames={Math.round(DURATION * FPS)} />
);
