resource "aws_instance" "this" {
  ami                    = data.aws_ssm_parameter.ubuntu_ami.insecure_value
  instance_type          = var.instance_type
  vpc_security_group_ids = [aws_security_group.ssh.id]

  # No subnet: EC2 picks any AZ in the region that has capacity. The
  # default subnets assign a public IP on launch.

  user_data = templatefile("${path.module}/cloud-init.yaml.tftpl", {
    ssh_public_key = trimspace(file(pathexpand(var.public_key_path)))
  })
  # One-time spot instances can't be stopped to apply new user data.
  user_data_replace_on_change = true

  instance_market_options {
    market_type = "spot"
    spot_options {
      spot_instance_type             = "one-time"
      instance_interruption_behavior = "terminate"
    }
  }

  root_block_device {
    volume_type           = "gp3"
    volume_size           = var.volume_size
    encrypted             = true
    delete_on_termination = true
  }

  metadata_options {
    http_tokens = "required"
  }

  tags = {
    Name = var.name
  }

  lifecycle {
    ignore_changes = [ami]
  }
}
