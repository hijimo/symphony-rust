import { create } from 'zustand';
import type { Project, PaginationData } from '../types';
import {
  getProjects,
  createProject as apiCreateProject,
  deleteProject as apiDeleteProject,
  startService as apiStartService,
  stopService as apiStopService,
  restartService as apiRestartService,
} from '../api/projects';
import type { GetProjectsParams, CreateProjectParams } from '../api/projects';

interface ProjectState {
  projects: Project[];
  currentProject: Project | null;
  loading: boolean;
  pagination: {
    pageNo: number;
    pageSize: number;
    totalCount: number;
    pages: number;
  };
  fetchProjects: (params: GetProjectsParams) => Promise<void>;
  createProject: (data: CreateProjectParams) => Promise<Project>;
  deleteProject: (id: number) => Promise<void>;
  startService: (id: number) => Promise<void>;
  stopService: (id: number) => Promise<void>;
  restartService: (id: number) => Promise<void>;
  setCurrentProject: (project: Project | null) => void;
  updateProjectInList: (id: number, updates: Partial<Project>) => void;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  currentProject: null,
  loading: false,
  pagination: {
    pageNo: 1,
    pageSize: 20,
    totalCount: 0,
    pages: 0,
  },

  fetchProjects: async (params) => {
    set({ loading: true });
    try {
      const data: PaginationData<Project> = await getProjects(params);
      set({
        projects: data.records,
        pagination: {
          pageNo: data.pageNo,
          pageSize: data.pageSize,
          totalCount: data.totalCount,
          pages: data.pages,
        },
      });
    } finally {
      set({ loading: false });
    }
  },

  createProject: async (data) => {
    const project = await apiCreateProject(data);
    return project;
  },

  deleteProject: async (id) => {
    await apiDeleteProject(id);
    const { projects } = get();
    set({ projects: projects.filter((p) => p.id !== id) });
  },

  startService: async (id) => {
    const result = await apiStartService(id);
    get().updateProjectInList(id, { service_status: result.status });
  },

  stopService: async (id) => {
    const result = await apiStopService(id);
    get().updateProjectInList(id, { service_status: result.status });
  },

  restartService: async (id) => {
    const result = await apiRestartService(id);
    get().updateProjectInList(id, { service_status: result.status });
  },

  setCurrentProject: (project) => {
    set({ currentProject: project });
  },

  updateProjectInList: (id, updates) => {
    const { projects } = get();
    set({
      projects: projects.map((p) => (p.id === id ? { ...p, ...updates } : p)),
    });
  },
}));
