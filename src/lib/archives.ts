export const ARCHIVE_EXTENSIONS = ["zip", "7z", "rar", "tar", "gz", "tgz"] as const

export const ARCHIVE_FILE_FILTER = {
  name: "Archive",
  extensions: [...ARCHIVE_EXTENSIONS],
}
