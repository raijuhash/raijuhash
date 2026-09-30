variable "name" {
  description = "Name prefix for all resources."
  type        = string
  default     = "raijuhash-x86"
}

variable "region" {
  description = "AWS region (Spain)."
  type        = string
  default     = "eu-south-2"
}

variable "instance_type" {
  description = "EC2 instance type."
  type        = string
  default     = "c8a.large"
}

variable "volume_size" {
  description = "Root EBS volume size in GiB."
  type        = number
  default     = 30
}

variable "public_key_path" {
  description = "Local SSH public key to authorize on the instance."
  type        = string
  default     = "~/.ssh/id_ecdsa.pub"
}
