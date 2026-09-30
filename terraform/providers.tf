provider "aws" {
  region = var.region

  # EC2 reports InsufficientInstanceCapacity as a 5xx, which the default 25
  # retries turn into minutes of silent backoff. Fail fast instead.
  max_retries = 5

  default_tags {
    tags = {
      Project = var.name
    }
  }
}
