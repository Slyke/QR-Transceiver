export const createSectionOrder = (defaults: Record<string, string[]>) => {
  const initial = Object.fromEntries(
    Object.entries(defaults).map(([page, ids]) => {
      try {
        const saved: unknown = JSON.parse(localStorage.getItem('qrtx-section-order:' + page) ?? 'null');
        if (Array.isArray(saved)) return [page, [...new Set([...saved.filter((id) => ids.includes(id)), ...ids])]];
      } catch {
        /* Keep the default order. */
      }
      return [page, [...ids]];
    })
  );
  let orders = $state<Record<string, string[]>>(initial);
  return {
    get: (page: string) => orders[page] ?? [],
    move: ({ page, id, direction, visible }: { page: string; id: string; direction: number; visible?: string[] }) => {
      const order = [...(orders[page] ?? [])];
      const shown = visible ?? order;
      const neighbor = shown[shown.indexOf(id) + direction];
      if (!neighbor) return;
      const from = order.indexOf(id);
      const to = order.indexOf(neighbor);
      [order[from], order[to]] = [order[to], order[from]];
      orders = { ...orders, [page]: order };
      try {
        localStorage.setItem('qrtx-section-order:' + page, JSON.stringify(order));
      } catch {
        /* Keep the session order. */
      }
    }
  };
};
