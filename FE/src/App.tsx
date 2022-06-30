import type { Component } from 'solid-js';

import logo from './logo.svg';
import styles from './App.module.css';
import RecordingScreen from './RecordingScreen';

const App: Component = () => {
  return (
    <div class={styles.App}>
      <RecordingScreen></RecordingScreen>
    </div>
  );
};

export default App;
