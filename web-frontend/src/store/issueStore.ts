import { create } from 'zustand';
import { aiGenerateIssue } from '../api/aiGenerate';
import type { AIGenerateRequest } from '../types/issue';

export type AIGenerateStatus = 'idle' | 'generating' | 'done' | 'error';

interface IssueState {
  // AI generation state
  aiStatus: AIGenerateStatus;
  generatedContent: string;
  aiError: string | null;
  abortController: AbortController | null;

  // Actions
  startGenerate: (projectId: number, data: AIGenerateRequest) => void;
  stopGenerate: () => void;
  resetAI: () => void;
  setGeneratedContent: (content: string) => void;
}

export const useIssueStore = create<IssueState>((set, get) => ({
  aiStatus: 'idle',
  generatedContent: '',
  aiError: null,
  abortController: null,

  startGenerate: (projectId, data) => {
    // Abort any existing generation
    const { abortController: existing } = get();
    if (existing) {
      existing.abort();
    }

    const controller = new AbortController();
    set({
      aiStatus: 'generating',
      generatedContent: '',
      aiError: null,
      abortController: controller,
    });

    aiGenerateIssue(
      projectId,
      data,
      {
        onChunk: (content) => {
          set((state) => ({
            generatedContent: state.generatedContent + content,
          }));
        },
        onDone: (fullContent) => {
          set({
            aiStatus: 'done',
            generatedContent: fullContent,
            abortController: null,
          });
        },
        onError: (error) => {
          set({
            aiStatus: 'error',
            aiError: error,
            abortController: null,
          });
        },
      },
      controller.signal,
    );
  },

  stopGenerate: () => {
    const { abortController } = get();
    if (abortController) {
      abortController.abort();
    }
    set({
      aiStatus: 'idle',
      abortController: null,
    });
  },

  resetAI: () => {
    const { abortController } = get();
    if (abortController) {
      abortController.abort();
    }
    set({
      aiStatus: 'idle',
      generatedContent: '',
      aiError: null,
      abortController: null,
    });
  },

  setGeneratedContent: (content) => {
    set({ generatedContent: content });
  },
}));
