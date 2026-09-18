import { invoke } from "../../invoke";
import type { DeleteJob, DeleteMode, DeletePreview, ProjectRow, Roots, RootsView, SessionDetail, SessionPage, SessionQuery } from "./types";

export const listSessions = (query: SessionQuery) => invoke<SessionPage>("sessions_list", { query });
export const listProjects = () => invoke<ProjectRow[]>("sessions_projects");
export const readSession = (id: string, offset: number) => invoke<SessionDetail>("sessions_read", { id, offset });
export const getRoots = () => invoke<RootsView>("sessions_roots");
export const setRoots = (overrides: Roots) => invoke<RootsView>("sessions_set_roots", { overrides });
export const setFavorite = (ids: string[], favorite: boolean) => invoke<void>("sessions_favorite", { ids, favorite });
export const openSession = (id: string, target: "folder" | "project" | "native" | "exports") => invoke<void>("sessions_open", { id, target });
export const previewDelete = (ids: string[], mode: DeleteMode) => invoke<DeletePreview>("sessions_delete_preview", { ids, mode });
export const executeDelete = (token: string) => invoke<DeleteJob>("sessions_delete_execute", { token });
export const deleteJob = () => invoke<DeleteJob | null>("sessions_job");
export const cancelDelete = () => invoke<void>("sessions_cancel");
