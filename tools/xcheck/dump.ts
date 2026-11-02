import { BLOCKS } from "./gsystem-editor/web/src/core/params"; // clone of davidavalanches/gsystem-editor
const P = (p: any) => [p.name, p.lo, p.hi, [...p.choices], p.choicesBase];
console.log(JSON.stringify(BLOCKS.map((b) => ({
  key: b.key, count: b.countField, size: b.size, enable: b.enableName,
  types: b.types.map((t) => [t.name, t.params.map(P)]), params: b.params.map(P),
}))));
