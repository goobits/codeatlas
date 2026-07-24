import "./component.svelte";
import ".";

export async function loadPlugin(name: string): Promise<unknown> {
  return import(`./${name}`);
}

void loadPlugin("plugin");
