# Ubuntu 26.04 LTS amd64, latest release published by Canonical.
data "aws_ssm_parameter" "ubuntu_ami" {
  name = "/aws/service/canonical/ubuntu/server/26.04/stable/current/amd64/hvm/ebs-gp3/ami-id"
}

data "aws_vpc" "default" {
  default = true
}

# Public IP of this machine, used to restrict SSH.
data "http" "my_ip" {
  url = "https://checkip.amazonaws.com"
}
