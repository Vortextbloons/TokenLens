export function nanoid(size = 10): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  let id = "";
  const arr = new Uint8Array(size);
  crypto.getRandomValues(arr);
  for (let i = 0; i < size; i++) id += chars[arr[i] % chars.length];
  return id;
}
