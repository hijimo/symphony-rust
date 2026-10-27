// Bundle Monaco locally instead of loading it from a public CDN, so the
// editor works offline / on intranets / under CSP (verify B I4).
import { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor';
import EditorWorker from 'monaco-editor/editor/editor.worker.start?worker';

(self as unknown as { MonacoEnvironment: unknown }).MonacoEnvironment = {
  getWorker: () => new EditorWorker(),
};

loader.config({ monaco });

export {};
