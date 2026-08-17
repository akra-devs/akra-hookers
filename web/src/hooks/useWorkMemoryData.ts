import { useCallback } from "react";
import { useQuery } from "@tanstack/react-query";

import type { ApiClient } from "../api";

export function useWorkMemoryData(
  client: ApiClient | null,
  projectId: number | undefined,
) {
  const revision = useQuery({
    queryKey: ["work-revision"],
    queryFn: () => client!.workRevision(),
    enabled: client !== null,
    refetchInterval: 500,
    retry: false,
  });
  const works = useQuery({
    queryKey: ["work-items", projectId ?? "all", revision.data],
    queryFn: () => client!.workItems(projectId),
    enabled: client !== null && revision.data !== undefined,
    placeholderData: (previous) => previous,
    retry: false,
  });
  const edges = useQuery({
    queryKey: ["work-edges", projectId ?? "all", revision.data],
    queryFn: () => client!.workEdges(projectId),
    enabled: client !== null && revision.data !== undefined,
    placeholderData: (previous) => previous,
    retry: false,
  });
  const refresh = useCallback(async () => {
    await Promise.all([revision.refetch(), works.refetch(), edges.refetch()]);
  }, [edges, revision, works]);

  return {
    revision,
    works,
    edges,
    refresh,
    isReady: works.data !== undefined && edges.data !== undefined,
    isError: revision.isError || works.isError || edges.isError,
  };
}
