<script context="module" lang="ts">
import { moduleThing } from "./moduleThing";
export const load = (): string => moduleThing;
export { helperValue } from "./helper";
</script>

<script lang="ts">
import { Child } from "./components";
import { helperValue } from "./helper";

const templateValue = helperValue;
const scriptOnlyUnused = "not safe to gate without template semantics";

void Child;
void scriptOnlyUnused;
void import("./lazy/Lazy.svelte");
</script>

<main>{templateValue}</main>
