import "./component.svelte";
import ".";
import "#root";
import "./feature/consumer";
import "./styles.css";
import config from "./config.json";
import rawAsset from "./asset.svg?raw";
import "$app/environment";
import "$env/static/public";
import type { PageData } from "./$types";

const pages = import.meta.glob("/src/pages/*.ts");
const content = import.meta.glob("/src/content/**/*.md");

export async function loadPlugin(name: string): Promise<unknown> {
  return import(`./plugins/${name}.ts`);
}

export async function loadUnknown(specifier: string): Promise<unknown> {
  return import(specifier);
}

void config;
void rawAsset;
void pages;
void content;
void (null as PageData | null);
void loadPlugin("plugin");
void loadUnknown("./unknown");
