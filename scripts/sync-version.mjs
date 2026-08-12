import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url))
const rootDirectory = path.resolve(scriptDirectory, "..")

function read(relativePath) {
  return fs.readFileSync(path.join(rootDirectory, relativePath), "utf8")
}

function write(relativePath, contents) {
  fs.writeFileSync(path.join(rootDirectory, relativePath), contents)
}

function writeJson(relativePath, value) {
  write(relativePath, JSON.stringify(value, null, 2) + "\n")
}

const version = read("VERSION").trim()
if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error("VERSION must contain a valid semantic version, received: " + version)
}

const packageJson = JSON.parse(read("package.json"))
const packageLock = JSON.parse(read("package-lock.json"))
const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"))
let changed = false

if (packageJson.version !== version) {
  packageJson.version = version
  writeJson("package.json", packageJson)
  changed = true
}

if (packageLock.version !== version || packageLock.packages?.[""]?.version !== version) {
  packageLock.version = version
  if (packageLock.packages?.[""]) {
    packageLock.packages[""].version = version
  }
  writeJson("package-lock.json", packageLock)
  changed = true
}

if (tauriConfig.version !== version) {
  tauriConfig.version = version
  writeJson("src-tauri/tauri.conf.json", tauriConfig)
  changed = true
}

const cargoToml = read("src-tauri/Cargo.toml")
const cargoTomlPattern = /(\[package\][\s\S]*?^version = ")[^"]+(")/m
if (!cargoTomlPattern.test(cargoToml)) {
  throw new Error("Could not find the application version in src-tauri/Cargo.toml")
}
const updatedCargoToml = cargoToml.replace(cargoTomlPattern, "$1" + version + "$2")
if (updatedCargoToml !== cargoToml) {
  write("src-tauri/Cargo.toml", updatedCargoToml)
  changed = true
}

const cargoLock = read("src-tauri/Cargo.lock")
const cargoLockPattern = /(\[\[package\]\]\r?\nname = "mrmmr"\r?\nversion = ")[^"]+(")/m
if (!cargoLockPattern.test(cargoLock)) {
  throw new Error("Could not find the application package in src-tauri/Cargo.lock")
}
const updatedCargoLock = cargoLock.replace(cargoLockPattern, "$1" + version + "$2")
if (updatedCargoLock !== cargoLock) {
  write("src-tauri/Cargo.lock", updatedCargoLock)
  changed = true
}

console.log((changed ? "Synchronized MRMMR version " : "MRMMR version ") + version + (changed ? "." : " is already synchronized."))
